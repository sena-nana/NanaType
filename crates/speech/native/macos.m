#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <Speech/Speech.h>

typedef void (*NTCallback)(void *, int, const char *);
typedef void (*NTRelease)(void *);
@interface NTDelivery : NSObject
@property(nonatomic) void *context;
@property(nonatomic) NTCallback callback;
@property(nonatomic) NTRelease releaseContext;
@end
@implementation NTDelivery
- (void)dealloc { if (_releaseContext) _releaseContext(_context); }
@end
@interface NTRecognition : NSObject
@property(nonatomic, strong) SFSpeechRecognizer *recognizer;
@property(nonatomic) BOOL ended;
@property(nonatomic, strong) SFSpeechAudioBufferRecognitionRequest *request;
@property(nonatomic, strong) SFSpeechRecognitionTask *task;
@property(nonatomic, strong) AVAudioFormat *format;
@end
@implementation NTRecognition
@end

int nt_microphone_permission(void) {
    @autoreleasepool {
        AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
        if (status == AVAuthorizationStatusAuthorized) return 1;
        if (status != AVAuthorizationStatusNotDetermined) return 0;
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block BOOL granted = NO;
        [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL ok) {
            granted = ok; dispatch_semaphore_signal(done);
        }];
        dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
        return granted ? 1 : 0;
    }
}
int nt_system_permission(void) {
    @autoreleasepool {
        SFSpeechRecognizerAuthorizationStatus status = [SFSpeechRecognizer authorizationStatus];
        if (status == SFSpeechRecognizerAuthorizationStatusAuthorized) return 1;
        if (status != SFSpeechRecognizerAuthorizationStatusNotDetermined) return 0;
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block BOOL granted = NO;
        [SFSpeechRecognizer requestAuthorization:^(SFSpeechRecognizerAuthorizationStatus s) {
            granted = s == SFSpeechRecognizerAuthorizationStatusAuthorized;
            dispatch_semaphore_signal(done);
        }];
        dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
        return granted ? 1 : 0;
    }
}
size_t nt_system_languages(char *buffer, size_t length) {
    @autoreleasepool {
        NSMutableArray *tags = [NSMutableArray array];
        for (NSLocale *locale in [SFSpeechRecognizer supportedLocales]) [tags addObject:locale.localeIdentifier];
        NSString *text = [[tags sortedArrayUsingSelector:@selector(compare:)] componentsJoinedByString:@"\n"];
        const char *bytes = text.UTF8String;
        size_t size = strlen(bytes);
        if (length > size) memcpy(buffer, bytes, size + 1);
        return size;
    }
}
int nt_system_available(const char *language) {
    @autoreleasepool {
        SFSpeechRecognizer *recognizer = [[SFSpeechRecognizer alloc] initWithLocale:[[NSLocale alloc] initWithLocaleIdentifier:@(language)]];
        return recognizer && recognizer.available;
    }
}
void *nt_system_create(const char *language, void *context, NTCallback callback, NTRelease releaseContext) {
    @autoreleasepool {
        NTDelivery *delivery = [NTDelivery new];
        delivery.context = context; delivery.callback = callback; delivery.releaseContext = releaseContext;
        SFSpeechRecognizer *recognizer = [[SFSpeechRecognizer alloc] initWithLocale:[[NSLocale alloc] initWithLocaleIdentifier:@(language)]];
        if (!recognizer || !recognizer.available) return NULL;
        NTRecognition *session = [NTRecognition new];
        session.recognizer = recognizer;
        session.format = [[AVAudioFormat alloc] initWithCommonFormat:AVAudioPCMFormatFloat32 sampleRate:16000 channels:1 interleaved:NO];
        session.request = [SFSpeechAudioBufferRecognitionRequest new];
        session.request.shouldReportPartialResults = YES;
        if (recognizer.supportsOnDeviceRecognition) session.request.requiresOnDeviceRecognition = YES;
        session.task = [recognizer recognitionTaskWithRequest:session.request resultHandler:^(SFSpeechRecognitionResult *result, NSError *error) {
            if (result) delivery.callback(delivery.context, result.final ? 1 : 0, result.bestTranscription.formattedString.UTF8String);
            // A final result may be accompanied by a cancellation error: don't emit both.
            if (error && !result.final) delivery.callback(delivery.context, 2, "speech-system-error");
        }];
        return (__bridge_retained void *)session;
    }
}
int nt_system_append(void *handle, const float *samples, size_t length) {
    @autoreleasepool {
        NTRecognition *session = (__bridge NTRecognition *)handle;
        AVAudioPCMBuffer *buffer = [[AVAudioPCMBuffer alloc] initWithPCMFormat:session.format frameCapacity:(AVAudioFrameCount)length];
        if (!buffer) return 0;
        buffer.frameLength = (AVAudioFrameCount)length;
        memcpy(buffer.floatChannelData[0], samples, length * sizeof(float));
        [session.request appendAudioPCMBuffer:buffer];
        return 1;
    }
}
void nt_system_end(void *handle) {
    @autoreleasepool {
        NTRecognition *session = (__bridge NTRecognition *)handle;
        if (!session.ended) { session.ended = YES; [session.request endAudio]; }
    }
}
void nt_system_poll(void *handle) { (void)handle; }
void nt_system_close(void *handle) {
    @autoreleasepool {
        NTRecognition *session = (__bridge_transfer NTRecognition *)handle;
        [session.task cancel];
        if (!session.ended) { session.ended = YES; [session.request endAudio]; }
    }
}
