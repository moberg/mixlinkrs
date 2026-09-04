//
//  VST3 hosting for MixLink's two send-effect slots.
//
//  Threading: everything except MixLinkVST3Process runs on the main thread.
//  MixLinkVST3Process runs on the CoreAudio IOProc thread and touches nothing
//  that allocates, locks or messages Objective-C.
//
#import "MixLinkVST3Bridge.h"

#import <Cocoa/Cocoa.h>

#include <algorithm>
#include <atomic>
#include <cstring>
#include <fcntl.h>
#include <libkern/OSByteOrder.h>
#include <mach-o/fat.h>
#include <mach-o/loader.h>
#include <mach/machine.h>
#include <string>
#include <sys/sysctl.h>
#include <sys/types.h>
#include <unistd.h>
#include <vector>

#include "pluginterfaces/base/funknownimpl.h"
#include "pluginterfaces/gui/iplugview.h"
#include "pluginterfaces/gui/iplugviewcontentscalesupport.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstprocesscontext.h"
#include "pluginterfaces/vst/vstspeaker.h"
#include "public.sdk/source/common/memorystream.h"
#include "public.sdk/source/vst/hosting/eventlist.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/hosting/parameterchanges.h"
#include "public.sdk/source/vst/hosting/plugprovider.h"
#include "public.sdk/source/vst/hosting/processdata.h"

using namespace Steinberg;
using namespace Steinberg::Vst;

NSString *const MixLinkVST3BundlePathKey = @"bundlePath";
NSString *const MixLinkVST3NameKey = @"name";
NSString *const MixLinkVST3ClassNameKey = @"className";
NSString *const MixLinkVST3ClassUIDKey = @"classUID";

namespace {

/// Identifies MixLink to plugins and vends IMessage/IAttributeList. Plugins may
/// hold on to it, so it outlives every instance.
FUnknown *sharedHostContext ()
{
	static HostApplication *host = [] {
		auto *app = new HostApplication ();
		PluginContextFactory::instance ().setPluginContext (app);
		return app;
	}();
	return host;
}

std::string uidString (const VST3::UID &uid)
{
	return uid.toString ();
}

} // namespace

namespace {

static MixLinkVST3StateDirtyBlock gStateDirtyHandler;

void notifyStateDirty (BOOL immediate)
{
	MixLinkVST3StateDirtyBlock handler = gStateDirtyHandler;
	if (handler == nil)
		return;
	if (NSThread.isMainThread)
		handler (immediate);
	else
		dispatch_async (dispatch_get_main_queue (), ^{
			if (gStateDirtyHandler)
				gStateDirtyHandler (immediate);
		});
}

} // namespace

void MixLinkVST3SetStateDirtyHandler (MixLinkVST3StateDirtyBlock handler)
{
	gStateDirtyHandler = handler ? [handler copy] : nil;
}

#pragma mark - Plug frame

@class MixLinkEditorWindowDelegate;

struct MixLinkVST3Instance;

namespace {

/// The host side of plugin-driven resizing. IPlugView requires that the window
/// is resized first and IPlugView::onSize is called afterwards, in that order.
class MixLinkPlugFrame : public U::Implements<U::Directly<IPlugFrame>>
{
public:
	explicit MixLinkPlugFrame (MixLinkVST3Instance *owner) : owner (owner) {}
	tresult PLUGIN_API resizeView (IPlugView *view, ViewRect *newSize) SMTG_OVERRIDE;

private:
	MixLinkVST3Instance *owner;
};

/// Plugins call this from the UI and sometimes from process. Soundtoys
/// Crystallizer (and others) dereference it with no null check.
class MixLinkComponentHandler : public U::Implements<U::Directly<IComponentHandler, IComponentHandler2>>
{
public:
	explicit MixLinkComponentHandler (MixLinkVST3Instance *owner) : owner (owner) {}

	tresult PLUGIN_API beginEdit (ParamID) SMTG_OVERRIDE { return kResultOk; }
	tresult PLUGIN_API performEdit (ParamID id, ParamValue value) SMTG_OVERRIDE;
	tresult PLUGIN_API endEdit (ParamID) SMTG_OVERRIDE
	{
		notifyStateDirty (NO);
		return kResultOk;
	}
	tresult PLUGIN_API restartComponent (int32) SMTG_OVERRIDE { return kResultOk; }
	tresult PLUGIN_API setDirty (TBool state) SMTG_OVERRIDE
	{
		if (state)
			notifyStateDirty (NO);
		return kResultOk;
	}
	tresult PLUGIN_API requestOpenEditor (FIDString) SMTG_OVERRIDE { return kResultFalse; }
	tresult PLUGIN_API startGroupEdit () SMTG_OVERRIDE { return kResultOk; }
	tresult PLUGIN_API finishGroupEdit () SMTG_OVERRIDE { return kResultOk; }

private:
	MixLinkVST3Instance *owner = nullptr;
};

} // namespace

#pragma mark - Instance

struct MixLinkVST3Instance
{
	VST3::Hosting::Module::Ptr module;
	IPtr<PlugProvider> provider;
	IPtr<IComponent> component;
	IPtr<IAudioProcessor> processor;
	IPtr<IEditController> controller;
	IPtr<IPlugView> view;
	IPtr<MixLinkPlugFrame> plugFrame;
	IPtr<MixLinkComponentHandler> handler;

	NSWindow *window = nil;
	MixLinkEditorWindowDelegate *windowDelegate = nil;
	/// True while the host is applying a plug-in-requested size, so windowDidResize
	/// does not call onSize re-entrantly.
	bool resizingEditor = false;

	HostProcessData data;
	ParameterChanges inputChanges;
	ParameterChanges outputChanges;
	EventList inputEvents;
	EventList outputEvents;
	ProcessContext context {};

	std::string displayName;
	std::string classUID;
	double sampleRate = 48000;
	int32 maxBlock = 512;
	int32 inputChannels = 0;
	int32 outputChannels = 0;
	bool processing = false;
	/// Raised around MixLinkVST3ProcessInstance so mix inserts can retire safely.
	std::atomic<bool> instanceBusy {false};

	static constexpr uint32_t kEditQueue = 128;
	struct PendingEdit
	{
		ParamID id = 0;
		ParamValue value = 0;
	};
	PendingEdit edits[kEditQueue] {};
	std::atomic<uint32_t> editWrite {0};
	std::atomic<uint32_t> editRead {0};

	void queueEdit (ParamID id, ParamValue value)
	{
		const uint32_t w = editWrite.load (std::memory_order_relaxed);
		const uint32_t next = (w + 1) % kEditQueue;
		if (next == editRead.load (std::memory_order_acquire))
			return;
		edits[w] = {id, value};
		editWrite.store (next, std::memory_order_release);
	}

	void drainEdits (ParameterChanges &changes)
	{
		uint32_t r = editRead.load (std::memory_order_relaxed);
		const uint32_t w = editWrite.load (std::memory_order_acquire);
		while (r != w)
		{
			const PendingEdit edit = edits[r];
			r = (r + 1) % kEditQueue;
			int32 index = 0;
			if (IParamValueQueue *queue = changes.addParameterData (edit.id, index))
			{
				int32 point = 0;
				queue->addPoint (0, edit.value, point);
			}
		}
		editRead.store (r, std::memory_order_release);
	}

	void teardownProcessing ()
	{
		if (processor && processing)
		{
			processor->setProcessing (false);
			processing = false;
		}
		if (component)
			component->setActive (false);
		data.unprepare ();
	}

	bool setupProcessing (double rate, int32 block)
	{
		if (!processor || !component)
			return false;
		teardownProcessing ();

		ProcessSetup setup {};
		setup.processMode = kRealtime;
		setup.symbolicSampleSize = kSample32;
		setup.maxSamplesPerBlock = block;
		setup.sampleRate = rate;
		if (processor->setupProcessing (setup) != kResultOk)
			return false;

		sampleRate = rate;
		maxBlock = block;

		// HostProcessData owns the bus buffers when a block size is passed, which
		// keeps every bus the plugin declares valid, including sidechains and aux
		// outputs MixLink never reads.
		if (!data.prepare (*component, block, kSample32))
			return false;
		data.numSamples = 0;
		data.symbolicSampleSize = kSample32;
		data.processMode = kRealtime;
		data.inputParameterChanges = &inputChanges;
		data.outputParameterChanges = &outputChanges;
		data.inputEvents = &inputEvents;
		data.outputEvents = &outputEvents;
		data.processContext = &context;

		inputChannels = data.numInputs > 0 ? data.inputs[0].numChannels : 0;
		outputChannels = data.numOutputs > 0 ? data.outputs[0].numChannels : 0;

		context = {};
		context.sampleRate = rate;
		context.tempo = 120;
		context.timeSigNumerator = 4;
		context.timeSigDenominator = 4;
		context.state = ProcessContext::kPlaying | ProcessContext::kContTimeValid
		                | ProcessContext::kProjectTimeMusicValid | ProcessContext::kTempoValid
		                | ProcessContext::kTimeSigValid;

		if (component->setActive (true) != kResultOk)
			return false;
		processor->setProcessing (true);
		processing = true;
		return true;
	}
};

namespace {

tresult MixLinkComponentHandler::performEdit (ParamID id, ParamValue value)
{
	if (owner)
		owner->queueEdit (id, value);
	return kResultOk;
}

} // namespace

#pragma mark - Editor window

@interface MixLinkEditorWindowDelegate : NSObject <NSWindowDelegate>
@property (nonatomic, assign) MixLinkVST3Instance *instance;
@end

@implementation MixLinkEditorWindowDelegate
- (void)windowDidResize:(NSNotification *)notification
{
	MixLinkVST3Instance *instance = self.instance;
	if (instance == nullptr || instance->view == nullptr || instance->window == nil
	    || instance->resizingEditor)
		return;
	NSSize size = instance->window.contentView.frame.size;
	ViewRect rect (0, 0, static_cast<int32> (size.width), static_cast<int32> (size.height));
	instance->view->checkSizeConstraint (&rect);
	instance->view->onSize (&rect);
}

- (void)windowWillClose:(NSNotification *)notification
{
	MixLinkVST3Instance *instance = self.instance;
	if (instance == nullptr)
		return;
	notifyStateDirty (YES);
	if (instance->view)
	{
		instance->view->setFrame (nullptr);
		instance->view->removed ();
		instance->view = nullptr;
	}
	instance->plugFrame = nullptr;
	instance->window.delegate = nil;
	instance->window = nil;
	instance->windowDelegate = nil;
}
@end

namespace {

tresult PLUGIN_API MixLinkPlugFrame::resizeView (IPlugView *view, ViewRect *newSize)
{
	if (owner == nullptr || newSize == nullptr || view == nullptr || owner->window == nil)
		return kResultFalse;
	if (owner->resizingEditor)
		return kResultTrue;
	const NSSize size = NSMakeSize (newSize->getWidth (), newSize->getHeight ());
	if (size.width < 1 || size.height < 1)
		return kResultFalse;
	// IPlugView: resize the platform window first, then onSize in the same stack.
	owner->resizingEditor = true;
	[owner->window setContentSize:size];
	[owner->window.contentView setFrameSize:size];
	view->onSize (newSize);
	owner->resizingEditor = false;
	return kResultTrue;
}

} // namespace

#pragma mark - Slots

namespace {

struct Slot
{
	std::atomic<MixLinkVST3Instance *> instance {nullptr};
	std::atomic<bool> bypassed {false};
	/// Raised by the audio thread around a process call so a retiring instance
	/// can be released only once the callback has certainly let go of it.
	std::atomic<bool> busy {false};
};

Slot gSlots[MixLinkVST3SlotCount];

void passthrough (const float *inL, const float *inR, float *outL, float *outR, uint32_t frames)
{
	if (inL && outL)
		std::memcpy (outL, inL, frames * sizeof (float));
	else if (outL)
		std::memset (outL, 0, frames * sizeof (float));
	if (inR && outR)
		std::memcpy (outR, inR, frames * sizeof (float));
	else if (outR)
		std::memset (outR, 0, frames * sizeof (float));
}

void processPlugin (MixLinkVST3Instance *plugin, BOOL bypassed,
                    const float *inL, const float *inR, float *outL, float *outR, uint32_t frames)
{
	if (plugin == nullptr || bypassed || !plugin->processing || plugin->outputChannels == 0
	    || frames > static_cast<uint32_t> (plugin->maxBlock))
	{
		passthrough (inL, inR, outL, outR, frames);
		return;
	}

	auto fill = [frames] (float *dst, const float *src) {
		if (dst == nullptr)
			return;
		if (src)
			std::memcpy (dst, src, frames * sizeof (float));
		else
			std::memset (dst, 0, frames * sizeof (float));
	};
	auto silence = [frames] (float **channels, int32 count) {
		if (channels == nullptr)
			return;
		for (int32 ch = 0; ch < count; ++ch)
			if (channels[ch])
				std::memset (channels[ch], 0, frames * sizeof (float));
	};

	float **in = plugin->data.numInputs > 0 ? plugin->data.inputs[0].channelBuffers32 : nullptr;
	float **out = plugin->data.numOutputs > 0 ? plugin->data.outputs[0].channelBuffers32 : nullptr;

	if (in != nullptr && plugin->inputChannels > 0)
	{
		if (plugin->inputChannels == 1)
		{
			if (in[0])
			{
				for (uint32_t i = 0; i < frames; ++i)
					in[0][i] = 0.5f * ((inL ? inL[i] : 0.f) + (inR ? inR[i] : 0.f));
			}
		}
		else
		{
			fill (in[0], inL);
			fill (in[1], inR);
			silence (in + 2, plugin->inputChannels - 2);
		}
	}
	for (int32 bus = 1; bus < plugin->data.numInputs; ++bus)
		silence (plugin->data.inputs[bus].channelBuffers32, plugin->data.inputs[bus].numChannels);

	plugin->data.numSamples = static_cast<int32> (frames);
	plugin->inputChanges.clearQueue ();
	plugin->drainEdits (plugin->inputChanges);
	plugin->outputChanges.clearQueue ();
	plugin->inputEvents.clear ();
	plugin->outputEvents.clear ();
	for (int32 bus = 0; bus < plugin->data.numInputs; ++bus)
		plugin->data.inputs[bus].silenceFlags = 0;
	for (int32 bus = 0; bus < plugin->data.numOutputs; ++bus)
		plugin->data.outputs[bus].silenceFlags = 0;

	tresult processed = kResultFalse;
	try
	{
		processed = plugin->processor->process (plugin->data);
	}
	catch (...)
	{
		processed = kResultFalse;
	}
	if (processed != kResultOk || out == nullptr)
	{
		passthrough (inL, inR, outL, outR, frames);
		return;
	}

	fill (outL, out[0]);
	fill (outR, out[plugin->outputChannels > 1 ? 1 : 0]);

	plugin->context.projectTimeSamples += frames;
	plugin->context.continousTimeSamples += frames;
	plugin->context.projectTimeMusic += (frames / plugin->sampleRate) * (plugin->context.tempo / 60.0);
}

} // namespace

void MixLinkVST3Process (uint32_t slot,
                         const float *inL,
                         const float *inR,
                         float *outL,
                         float *outR,
                         uint32_t frames)
{
	if (slot >= MixLinkVST3SlotCount || frames == 0)
		return;
	Slot &s = gSlots[slot];

	s.busy.store (true, std::memory_order_seq_cst);
	MixLinkVST3Instance *plugin = s.instance.load (std::memory_order_acquire);
	processPlugin (plugin, s.bypassed.load (std::memory_order_relaxed) ? YES : NO, inL, inR, outL, outR, frames);
	s.busy.store (false, std::memory_order_seq_cst);
}

void MixLinkVST3ProcessInstance (MixLinkVST3Ref instance,
                                 BOOL bypassed,
                                 const float *inL,
                                 const float *inR,
                                 float *outL,
                                 float *outR,
                                 uint32_t frames)
{
	if (frames == 0)
		return;
	if (instance != nullptr)
		instance->instanceBusy.store (true, std::memory_order_seq_cst);
	processPlugin (instance, bypassed, inL, inR, outL, outR, frames);
	if (instance != nullptr)
		instance->instanceBusy.store (false, std::memory_order_seq_cst);
}

void MixLinkVST3RetireInstance (MixLinkVST3Ref instance)
{
	if (instance == nullptr)
		return;
	for (int i = 0; i < 500 && instance->instanceBusy.load (std::memory_order_seq_cst); ++i)
		usleep (1000);
}

void MixLinkVST3SetTempo (MixLinkVST3Ref instance, double bpm)
{
	if (instance == nullptr)
		return;
	instance->context.tempo = bpm < 20 ? 120 : bpm;
}

uint32_t MixLinkVST3ParameterCount (MixLinkVST3Ref instance)
{
	if (instance == nullptr || instance->controller == nullptr)
		return 0;
	return static_cast<uint32_t> (instance->controller->getParameterCount ());
}

NSString *MixLinkVST3ParameterName (MixLinkVST3Ref instance, uint32_t index)
{
	if (instance == nullptr || instance->controller == nullptr)
		return nil;
	ParameterInfo info {};
	if (instance->controller->getParameterInfo (static_cast<int32> (index), info) != kResultOk)
		return nil;
	NSUInteger len = 0;
	while (len < 128 && info.title[len] != 0)
		++len;
	return [[NSString alloc] initWithCharacters:reinterpret_cast<const unichar *> (info.title) length:len];
}

uint32_t MixLinkVST3ParameterIDAt (MixLinkVST3Ref instance, uint32_t index)
{
	if (instance == nullptr || instance->controller == nullptr)
		return 0;
	ParameterInfo info {};
	if (instance->controller->getParameterInfo (static_cast<int32> (index), info) != kResultOk)
		return 0;
	return info.id;
}

double MixLinkVST3GetParameter (MixLinkVST3Ref instance, uint32_t paramID)
{
	if (instance == nullptr || instance->controller == nullptr)
		return 0;
	return instance->controller->getParamNormalized (paramID);
}

void MixLinkVST3SetParameter (MixLinkVST3Ref instance, uint32_t paramID, double valueNormalized)
{
	if (instance == nullptr)
		return;
	const ParamValue value = std::min (1.0, std::max (0.0, valueNormalized));
	if (instance->controller)
		instance->controller->setParamNormalized (paramID, value);
	instance->queueEdit (paramID, value);
}

MixLinkVST3Ref MixLinkVST3SlotExchange (uint32_t slot, MixLinkVST3Ref next)
{
	if (slot >= MixLinkVST3SlotCount)
		return nullptr;
	Slot &s = gSlots[slot];
	MixLinkVST3Instance *previous = s.instance.exchange (next, std::memory_order_release);
	if (previous != nullptr && previous != next)
	{
		// The audio thread may still be inside a process call that loaded the old
		// pointer. Wait it out; a block is at most a few milliseconds.
		for (int i = 0; i < 500 && s.busy.load (std::memory_order_seq_cst); ++i)
			usleep (1000);
	}
	return previous == next ? nullptr : previous;
}

void MixLinkVST3SlotSetBypass (uint32_t slot, BOOL bypassed)
{
	if (slot >= MixLinkVST3SlotCount)
		return;
	gSlots[slot].bypassed.store (bypassed == YES, std::memory_order_relaxed);
}

#pragma mark - Discovery

static BOOL debuggerIsAttached (void)
{
	int mib[4] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid ()};
	struct kinfo_proc info {};
	size_t size = sizeof (info);
	if (sysctl (mib, 4, &info, &size, nullptr, 0) != 0)
		return NO;
	return (info.kp_proc.p_flag & P_TRACED) != 0;
}

/// Soundtoys refuses to run under a debugger and posts a modal per plug-in.
static BOOL skipSoundtoysInThisRun (void)
{
#if DEBUG
	return YES;
#else
	return debuggerIsAttached ();
#endif
}

static BOOL looksLikeSoundtoys (NSString *path, NSString *name)
{
	NSString *hay = [[NSString stringWithFormat:@"%@ %@", path ?: @"", name ?: @""] lowercaseString];
	if ([hay containsString:@"soundtoy"])
		return YES;
	static NSArray<NSString *> *keys = @[
		@"crystallizer", @"decapitator", @"devil-loc", @"devilloc", @"echoboy",
		@"effectrack", @"effect rack", @"filterfreak", @"littlealterboy",
		@"little alterboy", @"littleplate", @"little plate", @"microshift",
		@"panman", @"phasemistress", @"primaltap", @"radiator", @"sie-q",
		@"sieq", @"superplate", @"tremolator"
	];
	for (NSString *key in keys)
	{
		if ([hay containsString:key])
			return YES;
	}
	return NO;
}

/// VST3 send candidates: tagged `Fx`, or an audio effect that is not an instrument.
static bool isSendEffectClass (const VST3::Hosting::ClassInfo &info)
{
	if (info.category () != kVstAudioEffectClass)
		return false;
	const std::string &sub = info.subCategoriesString ();
	if (sub.find ("Fx") != std::string::npos)
		return true;
	if (sub.find ("Instrument") != std::string::npos)
		return false;
	return true;
}

static NSString *vst3ExecutablePath (NSString *bundlePath)
{
	NSBundle *bundle = [NSBundle bundleWithPath:bundlePath];
	NSString *exe = bundle.executablePath;
	NSFileManager *fm = NSFileManager.defaultManager;
	if (exe.length > 0 && [fm fileExistsAtPath:exe])
		return exe;
	NSString *macos = [bundlePath stringByAppendingPathComponent:@"Contents/MacOS"];
	NSArray<NSString *> *names = [fm contentsOfDirectoryAtPath:macos error:nil];
	for (NSString *name in names)
	{
		NSString *path = [macos stringByAppendingPathComponent:name];
		BOOL dir = NO;
		if ([fm fileExistsAtPath:path isDirectory:&dir] && !dir)
			return path;
	}
	return nil;
}

static BOOL cpuTypeMatchesHost (cpu_type_t type)
{
#if defined(__arm64__)
	return type == CPU_TYPE_ARM64;
#elif defined(__x86_64__)
	return type == CPU_TYPE_X86_64;
#else
	(void)type;
	return YES;
#endif
}

/// Reads the Mach-O header; does not execute the binary. Intel-only plugs
/// cannot dlopen into an arm64 MixLink, and trying prints fatal-looking dyld
/// errors and can take the process down inside CFBundleLoadExecutable.
static BOOL executableHasHostArchitecture (NSString *exePath)
{
	int fd = open (exePath.fileSystemRepresentation, O_RDONLY);
	if (fd < 0)
		return YES;
	uint8_t buf[512];
	ssize_t n = read (fd, buf, sizeof (buf));
	close (fd);
	if (n < (ssize_t)sizeof (uint32_t))
		return YES;

	uint32_t magic = 0;
	memcpy (&magic, buf, sizeof (magic));

	auto matchesAt = [&] (cpu_type_t type, bool swap) {
		if (swap)
			type = OSSwapInt32 (type);
		return cpuTypeMatchesHost (type) ? YES : NO;
	};

	if (magic == MH_MAGIC_64 || magic == MH_CIGAM_64)
	{
		if (n < (ssize_t)sizeof (mach_header_64))
			return YES;
		mach_header_64 mh {};
		memcpy (&mh, buf, sizeof (mh));
		return matchesAt (mh.cputype, magic == MH_CIGAM_64);
	}
	if (magic == FAT_MAGIC || magic == FAT_CIGAM)
	{
		if (n < (ssize_t)sizeof (fat_header))
			return YES;
		fat_header fh {};
		memcpy (&fh, buf, sizeof (fh));
		const bool swap = (magic == FAT_CIGAM);
		uint32_t narch = swap ? OSSwapInt32 (fh.nfat_arch) : fh.nfat_arch;
		if (narch > 16)
			narch = 16;
		const size_t need = sizeof (fat_header) + narch * sizeof (fat_arch);
		if (n < (ssize_t)need)
			return YES;
		for (uint32_t i = 0; i < narch; ++i)
		{
			fat_arch arch {};
			memcpy (&arch, buf + sizeof (fat_header) + i * sizeof (fat_arch), sizeof (arch));
			if (matchesAt (arch.cputype, swap))
				return YES;
		}
		return NO;
	}
	if (magic == FAT_MAGIC_64 || magic == FAT_CIGAM_64)
	{
		if (n < (ssize_t)sizeof (fat_header))
			return YES;
		fat_header fh {};
		memcpy (&fh, buf, sizeof (fh));
		const bool swap = (magic == FAT_CIGAM_64);
		uint32_t narch = swap ? OSSwapInt32 (fh.nfat_arch) : fh.nfat_arch;
		if (narch > 16)
			narch = 16;
		const size_t need = sizeof (fat_header) + narch * sizeof (fat_arch_64);
		if (n < (ssize_t)need)
			return YES;
		for (uint32_t i = 0; i < narch; ++i)
		{
			fat_arch_64 arch {};
			memcpy (&arch, buf + sizeof (fat_header) + i * sizeof (fat_arch_64), sizeof (arch));
			if (matchesAt (arch.cputype, swap))
				return YES;
		}
		return NO;
	}
	return YES;
}

static BOOL bundleHasHostArchitecture (NSString *bundlePath)
{
	NSString *exe = vst3ExecutablePath (bundlePath);
	if (exe.length == 0)
		return NO;
	return executableHasHostArchitecture (exe);
}

static BOOL subcategoriesLookLikeSendEffect (NSString *joined)
{
	if ([joined containsString:@"Fx"])
		return YES;
	if ([joined containsString:@"Instrument"])
		return NO;
	return YES;
}

/// Best-effort, no code load. Unknown bundles stay in the list; MixLinkVST3Load
/// still rejects instruments. Loading every VST3 just to read class tags used to
/// pull Arturia / iZotope / PACE into MixLink's address space at launch.
static BOOL isLikelySendEffectBundle (NSString *path)
{
	NSString *jsonPath = [path stringByAppendingPathComponent:@"Contents/Resources/moduleinfo.json"];
	NSData *jsonData = [NSData dataWithContentsOfFile:jsonPath];
	if (jsonData)
	{
		id obj = [NSJSONSerialization JSONObjectWithData:jsonData options:0 error:nil];
		NSArray *classes = [obj isKindOfClass:NSDictionary.class] ? obj[@"Classes"] : nil;
		BOOL sawAudio = NO;
		BOOL keep = NO;
		for (id entry in classes)
		{
			if (![entry isKindOfClass:NSDictionary.class])
				continue;
			NSDictionary *info = entry;
			if (![info[@"Category"] isEqualToString:@"Audio Module Class"])
				continue;
			sawAudio = YES;
			id sub = info[@"Sub Categories"];
			NSString *joined = [sub isKindOfClass:NSArray.class]
			                       ? [(NSArray *)sub componentsJoinedByString:@","]
			                       : (sub ?: @"");
			if (subcategoriesLookLikeSendEffect (joined))
			{
				keep = YES;
				break;
			}
		}
		if (sawAudio)
			return keep;
	}

	NSArray *comps = [NSBundle bundleWithPath:path].infoDictionary[@"AudioComponents"];
	if ([comps isKindOfClass:NSArray.class] && comps.count > 0)
	{
		BOOL anyEffect = NO;
		BOOL onlyInstruments = YES;
		for (id entry in comps)
		{
			if (![entry isKindOfClass:NSDictionary.class])
				continue;
			NSString *type = entry[@"type"];
			if ([type isEqualToString:@"aumu"] || [type isEqualToString:@"aumj"]
			    || [type isEqualToString:@"augn"])
				continue;
			onlyInstruments = NO;
			if ([type isEqualToString:@"aufx"] || [type isEqualToString:@"aumf"]
			    || [type isEqualToString:@"auol"] || type.length == 0)
				anyEffect = YES;
		}
		if (anyEffect)
			return YES;
		if (onlyInstruments)
			return NO;
	}
	return YES;
}

NSArray<NSDictionary<NSString *, NSString *> *> *MixLinkVST3ScanPlugins (void)
{
	NSMutableArray *result = [NSMutableArray array];
	NSMutableSet<NSString *> *seen = [NSMutableSet set];
	NSFileManager *fm = NSFileManager.defaultManager;

	NSMutableArray<NSString *> *roots = [NSMutableArray array];
	for (NSString *base in @[@"/Library/Audio/Plug-Ins/VST3"])
		[roots addObject:base];
	NSString *home = NSHomeDirectory ();
	if (home.length > 0)
		[roots addObject:[home stringByAppendingPathComponent:@"Library/Audio/Plug-Ins/VST3"]];

	for (NSString *root in roots)
	{
		NSDirectoryEnumerator<NSString *> *e = [fm enumeratorAtPath:root];
		for (NSString *relative in e)
		{
			if (![relative.pathExtension isEqualToString:@"vst3"])
				continue;
			// A .vst3 is a bundle; never descend into one.
			[e skipDescendants];
			NSString *path = [root stringByAppendingPathComponent:relative];
			NSString *name = relative.lastPathComponent.stringByDeletingPathExtension;
			if ([seen containsObject:path])
				continue;
			[seen addObject:path];
			if (skipSoundtoysInThisRun () && looksLikeSoundtoys (path, name))
				continue;
			if (!bundleHasHostArchitecture (path))
				continue;
			if (!isLikelySendEffectBundle (path))
				continue;
			[result addObject:@{MixLinkVST3BundlePathKey : path, MixLinkVST3NameKey : name}];
		}
	}

	[result sortUsingComparator:^NSComparisonResult (NSDictionary *a, NSDictionary *b) {
		return [a[MixLinkVST3NameKey] localizedCaseInsensitiveCompare:b[MixLinkVST3NameKey]];
	}];
	return result;
}

#pragma mark - Load / unload

MixLinkVST3Ref MixLinkVST3Load (NSString *bundlePath,
                                NSString *classUID,
                                double sampleRate,
                                uint32_t maxBlockSize,
                                NSString **error)
{
	auto fail = [&] (NSString *message) -> MixLinkVST3Ref {
		if (error)
			*error = message;
		return nullptr;
	};
	if (bundlePath.length == 0)
		return fail (@"No plugin path");
	if (skipSoundtoysInThisRun ()
	    && looksLikeSoundtoys (bundlePath, bundlePath.lastPathComponent.stringByDeletingPathExtension))
		return fail (@"Soundtoys plugins are skipped while debugging");
	if (!bundleHasHostArchitecture (bundlePath))
		return fail (@"Incompatible architecture (plugin is Intel-only)");

	sharedHostContext ();

	// Soundtoys/PACE throw C++ `wmException` and sometimes NSException. catch
	// (...) alone is not enough once the throw has crossed an AppKit frame.
	@try
	{
	try
	{
		std::string loadError;
		auto module = VST3::Hosting::Module::create (bundlePath.fileSystemRepresentation, loadError);
		if (!module)
			return fail ([NSString stringWithFormat:@"%s", loadError.c_str ()]);

		const auto &factory = module->getFactory ();
		const VST3::Hosting::ClassInfo *chosen = nullptr;
		auto classInfos = factory.classInfos ();
		// Without an explicit UID, prefer an effect over an instrument: a bundle may
		// ship both, and only the effect is useful on a send.
		for (bool effectsOnly : {true, false})
		{
			for (const auto &info : classInfos)
			{
				if (info.category () != kVstAudioEffectClass)
					continue;
				if (classUID.length > 0 && uidString (info.ID ()) != classUID.UTF8String)
					continue;
				if (effectsOnly && info.subCategoriesString ().find ("Instrument") != std::string::npos)
					continue;
				chosen = &info;
				break;
			}
			if (chosen != nullptr)
				break;
		}
		if (chosen == nullptr)
			return fail (@"No matching audio effect in bundle");

		auto instance = std::unique_ptr<MixLinkVST3Instance> (new MixLinkVST3Instance ());
		instance->module = module;
		instance->provider = owned (new PlugProvider (factory, *chosen, true));
		if (!instance->provider->initialize ())
			return fail (@"Plugin failed to initialize");

		instance->component = instance->provider->getComponentPtr ();
		instance->controller = instance->provider->getControllerPtr ();
		if (!instance->component)
			return fail (@"Plugin has no component");
		instance->processor = FUnknownPtr<IAudioProcessor> (instance->component);
		if (!instance->processor)
			return fail (@"Plugin is not an audio processor");

		instance->handler = owned (new MixLinkComponentHandler (instance.get ()));
		if (instance->controller)
			instance->controller->setComponentHandler (instance->handler);

		// The spec requires one arrangement per bus the plugin declared. Passing a
		// single stereo pair to a multi-bus plugin (Soundtoys sidechains, etc.)
		// leaves it in a half-set-up state that process() then crashes in.
		const int32 inputBuses = instance->component->getBusCount (kAudio, kInput);
		const int32 outputBuses = instance->component->getBusCount (kAudio, kOutput);
		std::vector<SpeakerArrangement> inArr (static_cast<size_t> (std::max (inputBuses, 0)),
		                                       SpeakerArr::kStereo);
		std::vector<SpeakerArrangement> outArr (static_cast<size_t> (std::max (outputBuses, 0)),
		                                        SpeakerArr::kStereo);
		for (int32 i = 0; i < inputBuses; ++i)
			instance->processor->getBusArrangement (kInput, i, inArr[static_cast<size_t> (i)]);
		for (int32 i = 0; i < outputBuses; ++i)
			instance->processor->getBusArrangement (kOutput, i, outArr[static_cast<size_t> (i)]);
		if (inputBuses > 0 && inArr[0] == 0)
			inArr[0] = SpeakerArr::kStereo;
		if (outputBuses > 0 && outArr[0] == 0)
			outArr[0] = SpeakerArr::kStereo;
		instance->processor->setBusArrangements (inArr.data (), inputBuses, outArr.data (), outputBuses);

		// Only the first audio in/out bus carries the send; leaving sidechains and
		// extra outputs inactive keeps plugins from expecting data MixLink has none of.
		for (int32 i = 0; i < inputBuses; ++i)
			instance->component->activateBus (kAudio, kInput, i, i == 0);
		for (int32 i = 0; i < outputBuses; ++i)
			instance->component->activateBus (kAudio, kOutput, i, i == 0);
		for (int32 i = 0; i < instance->component->getBusCount (kEvent, kInput); ++i)
			instance->component->activateBus (kEvent, kInput, i, false);

		if (!instance->setupProcessing (sampleRate, static_cast<int32> (maxBlockSize)))
			return fail (@"Plugin rejected the audio setup");

		instance->displayName = chosen->name ();
		instance->classUID = uidString (chosen->ID ());
		return instance.release ();
	}
	catch (...)
	{
		return fail (@"Plugin crashed during load");
	}
	} @catch (NSException *ex) {
		return fail ([NSString stringWithFormat:@"%@: %@", ex.name, ex.reason ?: @"exception"]);
	} @catch (...) {
		return fail (@"Plugin crashed during load");
	}
}

void MixLinkVST3Unload (MixLinkVST3Ref instance)
{
	if (instance == nullptr)
		return;
	@try
	{
		MixLinkVST3CloseEditor (instance);
		instance->teardownProcessing ();
		if (instance->controller)
			instance->controller->setComponentHandler (nullptr);
		instance->handler = nullptr;
		// The component and controller references came from PlugProvider's own IPtrs,
		// so dropping ours and letting ~PlugProvider terminate is the whole teardown.
		instance->processor = nullptr;
		instance->controller = nullptr;
		instance->component = nullptr;
		instance->provider = nullptr;
		instance->module = nullptr;
		delete instance;
	} @catch (...) {
	}
}

NSString *MixLinkVST3DisplayName (MixLinkVST3Ref instance)
{
	if (instance == nullptr)
		return @"";
	return [NSString stringWithUTF8String:instance->displayName.c_str ()] ?: @"";
}

NSString *MixLinkVST3ClassUID (MixLinkVST3Ref instance)
{
	if (instance == nullptr)
		return @"";
	return [NSString stringWithUTF8String:instance->classUID.c_str ()] ?: @"";
}

NSArray<NSDictionary<NSString *, NSString *> *> *MixLinkVST3ClassesInModule (MixLinkVST3Ref instance)
{
	NSMutableArray *result = [NSMutableArray array];
	if (instance == nullptr || !instance->module)
		return result;
	@try
	{
		for (const auto &info : instance->module->getFactory ().classInfos ())
		{
			if (!isSendEffectClass (info))
				continue;
			NSString *name = [NSString stringWithUTF8String:info.name ().c_str ()] ?: @"";
			NSString *uid = [NSString stringWithUTF8String:uidString (info.ID ()).c_str ()] ?: @"";
			[result addObject:@{MixLinkVST3ClassNameKey : name, MixLinkVST3ClassUIDKey : uid}];
		}
	} @catch (...) {
	}
	return result;
}

BOOL MixLinkVST3Reconfigure (MixLinkVST3Ref instance, double sampleRate, uint32_t maxBlockSize)
{
	if (instance == nullptr)
		return NO;
	@try
	{
		return instance->setupProcessing (sampleRate, static_cast<int32> (maxBlockSize)) ? YES : NO;
	} @catch (...) {
		return NO;
	}
}

#pragma mark - Editor

BOOL MixLinkVST3HasEditor (MixLinkVST3Ref instance)
{
	// Do not call createView as a probe. Soundtoys Crystallizer (and several
	// other UIs) construct a full editor on that call and crash when the
	// unused IPlugView is released immediately.
	if (instance == nullptr || !instance->controller)
		return NO;
	return YES;
}

void MixLinkVST3ShowEditor (MixLinkVST3Ref instance, NSString *title)
{
	if (instance == nullptr || !instance->controller)
		return;
	if (instance->window != nil)
	{
		[instance->window makeKeyAndOrderFront:nil];
		return;
	}

	IPtr<IPlugView> view;
	try
	{
		view = owned (instance->controller->createView (ViewType::kEditor));
	}
	catch (...)
	{
		return;
	}
	if (!view || view->isPlatformTypeSupported (kPlatformTypeNSView) != kResultTrue)
		return;

	ViewRect rect {};
	if (view->getSize (&rect) != kResultOk || rect.getWidth () <= 0 || rect.getHeight () <= 0)
		rect = ViewRect (0, 0, 800, 500);

	NSRect content = NSMakeRect (0, 0, rect.getWidth (), rect.getHeight ());
	NSWindow *window =
	    [[NSWindow alloc] initWithContentRect:content
	                                styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
	                                          | NSWindowStyleMaskMiniaturizable
	                                          | NSWindowStyleMaskResizable
	                                  backing:NSBackingStoreBuffered
	                                    defer:NO];
	window.title = title.length > 0 ? title : MixLinkVST3DisplayName (instance);
	window.releasedWhenClosed = NO;
	NSView *container = [[NSView alloc] initWithFrame:content];
	container.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
	window.contentView = container;

	MixLinkEditorWindowDelegate *delegate = [MixLinkEditorWindowDelegate new];
	delegate.instance = instance;
	window.delegate = delegate;

	instance->plugFrame = owned (new MixLinkPlugFrame (instance));
	instance->view = view;
	instance->window = window;
	instance->windowDelegate = delegate;

	// attached() is allowed to call IPlugFrame::resizeView. That used to return
	// false because window was still nil, so Soundtoys EffectRack never grew to
	// include the preset bar and Input/Mix knobs.
	view->setFrame (instance->plugFrame);
	if (auto scale = FUnknownPtr<IPlugViewContentScaleSupport> (view.get ()))
		scale->setContentScaleFactor (static_cast<float> (window.backingScaleFactor));
	if (view->attached ((__bridge void *)container, kPlatformTypeNSView) != kResultOk)
	{
		view->setFrame (nullptr);
		instance->plugFrame = nullptr;
		instance->view = nullptr;
		instance->windowDelegate = nil;
		instance->window = nil;
		window.delegate = nil;
		return;
	}

	ViewRect now {};
	if (view->getSize (&now) == kResultOk && now.getWidth () > 0 && now.getHeight () > 0)
	{
		NSSize size = NSMakeSize (now.getWidth (), now.getHeight ());
		instance->resizingEditor = true;
		[window setContentSize:size];
		[container setFrameSize:size];
		view->onSize (&now);
		instance->resizingEditor = false;
	}

	[window center];
	[window makeKeyAndOrderFront:nil];
}

void MixLinkVST3CloseEditor (MixLinkVST3Ref instance)
{
	if (instance == nullptr || instance->window == nil)
		return;
	[instance->window close];
}

#pragma mark - State

// Sidecar layout: magic, version, then length-prefixed component and controller
// blobs. Keeping both in one file means a slot restores in a single read.
static const uint32_t kStateMagic = 0x4D4C5633; // 'MLV3'
static const uint32_t kStateVersion = 1;

NSData *MixLinkVST3SaveState (MixLinkVST3Ref instance)
{
	if (instance == nullptr || !instance->component)
		return nil;

	@try
	{
		MemoryStream componentState;
		if (instance->component)
			instance->component->getState (&componentState);
		MemoryStream controllerState;
		if (instance->controller)
			instance->controller->getState (&controllerState);
		if (componentState.getSize () == 0 && controllerState.getSize () == 0)
			return nil;

		NSMutableData *out = [NSMutableData data];
		uint32_t header[2] = {kStateMagic, kStateVersion};
		[out appendBytes:header length:sizeof (header)];
		for (MemoryStream *stream : {&componentState, &controllerState})
		{
			uint32_t size = static_cast<uint32_t> (stream->getSize ());
			[out appendBytes:&size length:sizeof (size)];
			if (size > 0)
				[out appendBytes:stream->getData () length:size];
		}
		return out;
	} @catch (...) {
		return nil;
	}
}

BOOL MixLinkVST3RestoreState (MixLinkVST3Ref instance, NSData *state)
{
	if (instance == nullptr || !instance->component || state.length < 16)
		return NO;

	@try
	{
		const uint8_t *bytes = static_cast<const uint8_t *> (state.bytes);
		uint32_t magic = 0, version = 0;
		std::memcpy (&magic, bytes, 4);
		std::memcpy (&version, bytes + 4, 4);
		if (magic != kStateMagic || version != kStateVersion)
			return NO;

		NSUInteger cursor = 8;
		std::vector<std::vector<uint8_t>> blobs;
		for (int i = 0; i < 2; ++i)
		{
			if (cursor + 4 > state.length)
				return NO;
			uint32_t size = 0;
			std::memcpy (&size, bytes + cursor, 4);
			cursor += 4;
			if (cursor + size > state.length)
				return NO;
			blobs.emplace_back (bytes + cursor, bytes + cursor + size);
			cursor += size;
		}

		const bool wasProcessing = instance->processing;
		const double rate = instance->sampleRate;
		const int32 block = instance->maxBlock;
		if (wasProcessing)
			instance->teardownProcessing ();

		if (!blobs[0].empty ())
		{
			MemoryStream componentState (blobs[0].data (), static_cast<TSize> (blobs[0].size ()));
			instance->component->setState (&componentState);
			if (instance->controller)
			{
				int64 ignored = 0;
				componentState.seek (0, IBStream::kIBSeekSet, &ignored);
				instance->controller->setComponentState (&componentState);
			}
		}
		if (!blobs[1].empty () && instance->controller)
		{
			MemoryStream controllerState (blobs[1].data (), static_cast<TSize> (blobs[1].size ()));
			instance->controller->setState (&controllerState);
		}

		if (wasProcessing)
			instance->setupProcessing (rate, block);
		return YES;
	} @catch (...) {
		return NO;
	}
}
