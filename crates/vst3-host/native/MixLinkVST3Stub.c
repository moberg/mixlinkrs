#include <stdint.h>
#include <string.h>

typedef void *MixLinkVST3Ref;

void MixLinkVST3Process(uint32_t slot, const float *inL, const float *inR, float *outL, float *outR, uint32_t frames) {
    (void)slot;
    if (frames == 0) return;
    if (outL && inL) memcpy(outL, inL, frames * sizeof(float));
    else if (outL) memset(outL, 0, frames * sizeof(float));
    if (outR && inR) memcpy(outR, inR, frames * sizeof(float));
    else if (outR) memset(outR, 0, frames * sizeof(float));
}

void MixLinkVST3ProcessInstance(MixLinkVST3Ref instance, int bypassed, const float *inL, const float *inR, float *outL, float *outR, uint32_t frames) {
    (void)instance;
    (void)bypassed;
    MixLinkVST3Process(0, inL, inR, outL, outR, frames);
}

void MixLinkVST3RetireInstance(MixLinkVST3Ref instance) { (void)instance; }
void MixLinkVST3SlotSetBypass(uint32_t slot, int bypassed) { (void)slot; (void)bypassed; }
void *MixLinkVST3SlotExchange(uint32_t slot, void *next) { (void)slot; return next; }
void MixLinkVST3Unload(void *instance) { (void)instance; }
void MixLinkVST3ShowEditor(void *instance, const void *title) { (void)instance; (void)title; }
void MixLinkVST3CloseEditor(void *instance) { (void)instance; }
void *MixLinkVST3CaptureEditor(void *instance) { (void)instance; return 0; }
void MixLinkVST3SetTempo(void *instance, double bpm) { (void)instance; (void)bpm; }
void MixLinkVST3SetParameter(void *instance, uint32_t id, double v) { (void)instance; (void)id; (void)v; }
uint32_t MixLinkVST3ParameterCount(void *instance) { (void)instance; return 0; }
double MixLinkVST3GetParameter(void *instance, uint32_t id) { (void)instance; (void)id; return 0; }
uint32_t MixLinkVST3ParameterIDAt(void *instance, uint32_t index) { (void)instance; (void)index; return 0; }
void *MixLinkVST3Load(const void *bundle, const void *uid, double sr, uint32_t block, const void **err) {
    (void)bundle; (void)uid; (void)sr; (void)block; (void)err;
    return 0;
}
const void *MixLinkVST3ScanPlugins(void) { return 0; }
const void *MixLinkVST3DisplayName(void *instance) { (void)instance; return 0; }
int MixLinkVST3HasEditor(void *instance) { (void)instance; return 0; }
void *MixLinkVST3SaveState(void *instance) { (void)instance; return 0; }
int MixLinkVST3RestoreState(void *instance, const void *data) { (void)instance; (void)data; return 0; }
void MixLinkVST3SetStateDirtyHandler(void (*fn)(void *, int)) { (void)fn; }
int MixLinkVST3Reconfigure(void *instance, double sr, uint32_t block) { (void)instance; (void)sr; (void)block; return 1; }
int MixLinkVST3Activate(void *instance) { (void)instance; return 1; }
