//
//  Objective-C facade over the VST3 SDK. Deliberately free of C++ so it can be
//  pulled into the Swift bridging header.
//
#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

#ifdef __cplusplus
// The implementation is Objective-C++, so without this the definitions would be
// C++-mangled and Swift could not find them.
extern "C" {
#endif

/// Keys in the dictionaries returned by `MixLinkVST3ScanPlugins`.
extern NSString *const MixLinkVST3BundlePathKey;
extern NSString *const MixLinkVST3NameKey;

/// Keys in the dictionaries returned by `MixLinkVST3ClassesInModule`.
extern NSString *const MixLinkVST3ClassNameKey;
extern NSString *const MixLinkVST3ClassUIDKey;

/// One loaded plugin. Opaque; owned by the caller until handed to a slot.
typedef struct MixLinkVST3Instance *MixLinkVST3Ref;

/// Number of plugin slots the audio thread can process.
#define MixLinkVST3SlotCount 8

#pragma mark - Discovery

/// Bundles in the system and user VST3 folders that look like send effects.
/// Uses bundle metadata and Mach-O headers only — does not load plugin code.
/// Intel-only binaries and AU instruments are omitted. Sorted by name.
NSArray<NSDictionary<NSString *, NSString *> *> *MixLinkVST3ScanPlugins(void);

#pragma mark - Lifetime (main thread)

/// Loads a bundle and instantiates an audio effect. `classUID` picks one class
/// out of a multi-effect bundle; pass nil for the first audio effect found.
MixLinkVST3Ref _Nullable MixLinkVST3Load(NSString *bundlePath,
                                         NSString *_Nullable classUID,
                                         double sampleRate,
                                         uint32_t maxBlockSize,
                                         NSString *_Nullable *_Nullable error);

/// Deactivates and releases the instance. Must not be called while the instance
/// is published to a slot; use `MixLinkVST3SlotExchange` to retire it first.
void MixLinkVST3Unload(MixLinkVST3Ref instance);

NSString *MixLinkVST3DisplayName(MixLinkVST3Ref instance);
NSString *MixLinkVST3ClassUID(MixLinkVST3Ref instance);

/// Audio effect classes exposed by the already-open module behind `instance`.
NSArray<NSDictionary<NSString *, NSString *> *> *MixLinkVST3ClassesInModule(MixLinkVST3Ref instance);

/// Re-runs `setupProcessing` for a new device rate or block size.
BOOL MixLinkVST3Reconfigure(MixLinkVST3Ref instance, double sampleRate, uint32_t maxBlockSize);

#pragma mark - Editor (main thread)

BOOL MixLinkVST3HasEditor(MixLinkVST3Ref instance);
void MixLinkVST3ShowEditor(MixLinkVST3Ref instance, NSString *title);
void MixLinkVST3CloseEditor(MixLinkVST3Ref instance);

#pragma mark - State (main thread)

NSData *_Nullable MixLinkVST3SaveState(MixLinkVST3Ref instance);
BOOL MixLinkVST3RestoreState(MixLinkVST3Ref instance, NSData *state);

/// Called on the main thread when a plugin wants its state written. `immediate`
/// is true when the editor closed; otherwise the host may coalesce.
typedef void (*MixLinkVST3StateDirtyFn)(MixLinkVST3Ref instance, int immediate);
void MixLinkVST3SetStateDirtyHandler(MixLinkVST3StateDirtyFn _Nullable fn);

#pragma mark - Slots

/// Publishes `next` to `slot` and returns whatever was there before, by then
/// guaranteed to be out of the audio thread's hands and safe to unload. Blocks
/// briefly, so main thread only.
MixLinkVST3Ref _Nullable MixLinkVST3SlotExchange(uint32_t slot, MixLinkVST3Ref _Nullable next);

void MixLinkVST3SlotSetBypass(uint32_t slot, BOOL bypassed);

/// Process a loaded instance that is not published to an IOProc slot.
/// Mix-page inserts use this from the mix render thread.
void MixLinkVST3ProcessInstance(MixLinkVST3Ref _Nullable instance,
                                BOOL bypassed,
                                const float *inL,
                                const float *inR,
                                float *outL,
                                float *outR,
                                uint32_t frames);

/// Wait until `instance` is out of `MixLinkVST3ProcessInstance`, then return it
/// so the caller may unload. Main thread only. Pass the pointer that was
/// published to the RT schedule, after swapping it out.
void MixLinkVST3RetireInstance(MixLinkVST3Ref _Nullable instance);

void MixLinkVST3SetTempo(MixLinkVST3Ref instance, double bpm);

uint32_t MixLinkVST3ParameterCount(MixLinkVST3Ref instance);
NSString *_Nullable MixLinkVST3ParameterName(MixLinkVST3Ref instance, uint32_t index);
uint32_t MixLinkVST3ParameterIDAt(MixLinkVST3Ref instance, uint32_t index);
double MixLinkVST3GetParameter(MixLinkVST3Ref instance, uint32_t paramID);
void MixLinkVST3SetParameter(MixLinkVST3Ref instance, uint32_t paramID, double valueNormalized);

/// Audio thread entry: allocation-free, lock-free and free of Objective-C.
/// Copies input to output when the slot is empty or bypassed. Reads the slot's
/// instance pointer once, so a concurrent exchange cannot tear the block.
void MixLinkVST3Process(uint32_t slot,
                        const float *inL,
                        const float *inR,
                        float *outL,
                        float *outR,
                        uint32_t frames);

#ifdef __cplusplus
} // extern "C"
#endif

NS_ASSUME_NONNULL_END
