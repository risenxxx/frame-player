// Fullscreen on macOS, one path for every way in (fullscreen.rs).
//
// - `toggleFullScreen:` with a sender (the green button, the Window menu's item, its shortcut) is handed
//   to the page, which then takes the same path as a double-click, F or Esc: the interface away, the
//   request two frames later. A call with no sender is Skathi's own `setFullscreen` and goes through.
// - `custom`: AppKit's custom-window hooks, so AppKit animates no snapshot: the live window's frame is
//   animated to the screen and back, and the picture keeps playing in it (mpv's way).
#import <Cocoa/Cocoa.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>

static void (*g_request)(void);
static void (*g_step)(void);
static int g_custom;
static NSRect g_windowed;
static IMP g_toggle;

static void fp_toggle(NSWindow *self, SEL cmd, id sender) {
    if (sender && g_request) {
        g_request();
        return;
    }
    ((void (*)(id, SEL, id))g_toggle)(self, cmd, sender);
}

static NSArray *fp_custom_windows(NSWindow *self, SEL cmd, NSWindow *window) {
    return g_custom ? @[ window ] : nil;
}

// The way out is ours too, as mpv's (video/out/mac/window.swift): AppKit's animates a snapshot of the
// fullscreen window, letterbox bars and all, scaled into the window, and the live picture then snapped to
// the window's size (+65 pt at 1080p60, measured).
static NSArray *fp_custom_exit_windows(NSWindow *self, SEL cmd, NSWindow *window) {
    return g_custom ? @[ window ] : nil;
}

// One step per display frame, from a CADisplayLink on the window's view at the display's rate (120 Hz on a
// ProMotion display), eased by the time the frame will be on screen (`targetTimestamp`). An NSTimer at 1/120 s
// did this before: its steps came a median 10-14 ms apart and 35-56 % of them missed a display frame (the app
// in the background has its timers coalesced), so the picture and its bars moved in uneven jerks (measured:
// held frames on the composed picture's edge). A latency-critical
// activity is held for the transition, and a 30 Hz timer steps only if the display link stops ticking.
// `animator` on the window's frame did not animate here (measured: the window was at its end frame within
// ~65 ms).
static NSTimer *g_timer;
static CADisplayLink *g_link;
static id g_activity;
// From the start of a custom transition to the end of its animation (fp_constrain).
static BOOL g_free;
@interface FPStepper : NSObject
@property(nonatomic) NSWindow *window;
@property(nonatomic) NSRect from, to;
@property(nonatomic) CFTimeInterval t0, duration, last;
- (void)tick:(CADisplayLink *)link;
- (void)stepAt:(CFTimeInterval)when;
@end
static FPStepper *g_stepper;
static void fp_stop(void) {
    [g_link invalidate];
    g_link = nil;
    [g_timer invalidate];
    g_timer = nil;
    if (g_activity) [NSProcessInfo.processInfo endActivity:g_activity];
    g_activity = nil;
    g_stepper = nil;
}
@implementation FPStepper
- (void)tick:(CADisplayLink *)link {
    self.last = CACurrentMediaTime();
    [self stepAt:link.targetTimestamp];
}
- (void)stepAt:(CFTimeInterval)when {
    NSWindow *window = self.window;
    NSRect from = self.from, to = self.to;
    double p = (when - self.t0) / self.duration;
    if (p >= 1.0) p = 1.0;
    if (p < 0) p = 0;
    double e = p < 0.5 ? 2 * p * p : 1 - 2 * (1 - p) * (1 - p);
    NSRect r = NSMakeRect(round(from.origin.x + (to.origin.x - from.origin.x) * e),
                          round(from.origin.y + (to.origin.y - from.origin.y) * e),
                          round(from.size.width + (to.size.width - from.size.width) * e),
                          round(from.size.height + (to.size.height - from.size.height) * e));
    // The last step is drawn at once: AppKit posts its Did only after a frame of the window at its end size,
    // and with nothing drawn it waited up to ~1 s (measured).
    if (!NSEqualRects(r, window.frame)) [window setFrame:r display:p >= 1.0];
    if (p >= 1.0) [window display];
    if (g_step) g_step();
    if (p >= 1.0) {
        g_free = NO;
        fp_stop();
    }
}
@end

static void fp_animate(NSWindow *window, NSRect from, NSRect to, NSTimeInterval duration) {
    fp_stop();
    g_activity = [NSProcessInfo.processInfo beginActivityWithOptions:NSActivityLatencyCritical | NSActivityUserInitiated
                                                              reason:@"fullscreen transition"];
    FPStepper *st = [FPStepper new];
    st.window = window;
    st.from = from;
    st.to = to;
    st.duration = duration;
    st.t0 = st.last = CACurrentMediaTime();
    g_stepper = st;
    g_link = [window.contentView displayLinkWithTarget:st selector:@selector(tick:)];
    g_link.preferredFrameRateRange = CAFrameRateRangeMake(60, 120, 120);
    [g_link addToRunLoop:NSRunLoop.mainRunLoop forMode:NSRunLoopCommonModes];
    g_timer = [NSTimer timerWithTimeInterval:1.0 / 30.0
                                     repeats:YES
                                       block:^(NSTimer *timer) {
                                         FPStepper *s = g_stepper;
                                         if (s && CACurrentMediaTime() - s.last > 0.05) {
                                             s.last = CACurrentMediaTime();
                                             [s stepAt:CACurrentMediaTime()];
                                         }
                                       }];
    [[NSRunLoop mainRunLoop] addTimer:g_timer forMode:NSRunLoopCommonModes];
}

// FPUI_FS_MOVE: `layer` (default) moves the picture, not the window: the window takes its end frame at once
// (it is transparent, and the page is hidden for the transition, child.rs `freeze`) and the video layer, black
// behind the held picture, is animated by Core Animation from the old box to the new one, in the render server
// at the display's rate. `window`: the window's frame stepped once per display frame from the main thread, the
// earlier way, whose steps missed 35-56 % of the display frames (measured).
static BOOL fp_layer_mode(void) {
    const char *e = getenv("FPUI_FS_MOVE");
    return !(e && strcmp(e, "window") == 0);
}

static NSView *fp_video_view(NSWindow *w) {
    NSView *content = w.contentView;
    for (NSView *sup in @[ content.superview ?: content, content ])
        for (NSView *v in sup.subviews)
            if ([v.layer isKindOfClass:[CAMetalLayer class]] && ![NSStringFromClass([v class]) hasPrefix:@"GPUI"]) return v;
    return nil;
}

// The video view from `from` to `to` (its superview's coordinates) over `duration`, ease in and out; `done`
// runs when the animation is over.
// `before` runs inside the same transaction, first: the window's frame change (AppKit ties it to the commit),
// so the window's new frame and the layer's start are on screen in the same frame. Without it the enter
// showed the picture at the wrong place for two frames (measured).
static void fp_layer_move(NSView *v, NSRect from, NSRect to, NSTimeInterval duration, void (^before)(void), void (^done)(void)) {
    CALayer *l = v.layer;
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    v.autoresizingMask = 0;
    if (before) before();
    v.frame = to;
    CGPoint ap = l.anchorPoint;
    CGRect fb = CGRectMake(0, 0, from.size.width, from.size.height), tb = CGRectMake(0, 0, to.size.width, to.size.height);
    CGPoint fp = CGPointMake(from.origin.x + ap.x * from.size.width, from.origin.y + ap.y * from.size.height);
    CGPoint tp = CGPointMake(to.origin.x + ap.x * to.size.width, to.origin.y + ap.y * to.size.height);
    [CATransaction setCompletionBlock:done];
    CABasicAnimation *b = [CABasicAnimation animationWithKeyPath:@"bounds"];
    b.fromValue = [NSValue valueWithRect:NSRectFromCGRect(fb)];
    b.toValue = [NSValue valueWithRect:NSRectFromCGRect(tb)];
    CABasicAnimation *p = [CABasicAnimation animationWithKeyPath:@"position"];
    p.fromValue = [NSValue valueWithPoint:NSPointFromCGPoint(fp)];
    p.toValue = [NSValue valueWithPoint:NSPointFromCGPoint(tp)];
    CAAnimationGroup *g = [CAAnimationGroup animation];
    g.animations = @[ b, p ];
    g.duration = duration;
    g.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
    [l addAnimation:g forKey:@"fpui-fs"];
    [CATransaction commit];
}

// The length of AppKit's animated resize while our way out runs it (fp_start_exit), else AppKit's own.
static NSTimeInterval g_resize_time;
static IMP g_resize_imp;
static NSTimeInterval fp_resize_time(NSWindow *self, SEL cmd, NSRect r) {
    if (g_resize_time > 0) return g_resize_time;
    return ((NSTimeInterval(*)(id, SEL, NSRect))g_resize_imp)(self, cmd, r);
}

static NSRect fp_in_window(NSRect screenRect, NSRect windowFrame) {
    return NSMakeRect(screenRect.origin.x - windowFrame.origin.x, screenRect.origin.y - windowFrame.origin.y, screenRect.size.width, screenRect.size.height);
}

static void fp_start_enter(NSWindow *self, SEL cmd, NSWindow *window, NSTimeInterval duration) {
    g_windowed = window.frame;
    g_free = YES;
    // Setting the mask puts the window at its fullscreen frame at once, which is the target: below the
    // notch (1728x1084 on a 14-inch MacBook Pro), not the screen's frame, whose extra 33 pt made the picture
    // drop ~17 pt when AppKit settled the window after Did (measured). It starts again from where it was.
    [window setStyleMask:window.styleMask | NSWindowStyleMaskFullScreen];
    NSRect target = window.frame;
    if (NSEqualRects(target, g_windowed)) target = window.screen.frame;
    NSView *v = fp_layer_mode() ? fp_video_view(window) : nil;
    if (v) {
        NSRect full = NSMakeRect(0, 0, target.size.width, target.size.height);
        fp_layer_move(v, fp_in_window(g_windowed, target), full, duration, ^{
          [window setFrame:target display:YES];
        }, ^{
          g_free = NO;
        });
        return;
    }
    [window setFrame:g_windowed display:YES];
    fp_animate(window, g_windowed, target, duration);
}

static NSRect fp_aspect_fit(NSSize s, NSRect in) {
    double k = MIN(in.size.width / s.width, in.size.height / s.height);
    double w = round(s.width * k), h = round(s.height * k);
    return NSMakeRect(round(NSMidX(in) - w / 2), round(NSMidY(in) - h / 2), w, h);
}

static void fp_start_exit(NSWindow *self, SEL cmd, NSWindow *window, NSTimeInterval duration) {
    if (NSIsEmptyRect(g_windowed)) {
        [window setStyleMask:window.styleMask & ~NSWindowStyleMaskFullScreen];
        return;
    }
    g_free = YES;
    NSView *v = fp_layer_mode() ? fp_video_view(window) : nil;
    if (v) {
        // The mask comes off at once, so the Space switch starts with the animation and the desktop shows around
        // the picture; then the window itself shrinks from the screen's frame straight to the windowed one, the
        // layer following it and filling it with the held fullscreen drawable: the picture keeps its aspect and
        // its bars shrink away along the way. Animating the layer and keeping the mask to the end left the
        // fullscreen Space's black above and below the picture until then; with the mask off, a layer
        // animation is not shown at all during the Space switch, only the window's frame is (measured).
        // AppKit's own animated resize (`animationResizeTime:` = the transition's) runs its steps in a loop of
        // its own, nothing else between them. The title bar is transparent meanwhile.
        NSRect full = window.frame;
        NSView *titlebar = [window standardWindowButton:NSWindowCloseButton].superview;
        CGFloat titleAlpha = titlebar.alphaValue;
        [CATransaction begin];
        [CATransaction setDisableActions:YES];
        titlebar.alphaValue = 0;
        [window setStyleMask:window.styleMask & ~NSWindowStyleMaskFullScreen];
        [window setFrame:full display:NO];
        [CATransaction commit];
        CFRunLoopPerformBlock(CFRunLoopGetMain(), kCFRunLoopCommonModes, ^{
          g_resize_time = duration * 0.95;
          [window setFrame:g_windowed display:YES animate:YES];
          g_resize_time = 0;
          titlebar.alphaValue = titleAlpha;
          g_free = NO;
          if (g_step) g_step();
        });
        CFRunLoopWakeUp(CFRunLoopGetMain());
        return;
    }
    // As mpv: first, at once, the window to the windowed shape fitted into the screen (the picture's own
    // box when the window has the video's shape: no bars), then the mask off and the animation from there.
    NSRect intermediate = fp_aspect_fit(g_windowed.size, window.frame);
    [window setFrame:intermediate display:YES];
    if (g_step) g_step();
    // On the run loop in its common modes, not the main dispatch queue: queued there, behind gpui's own
    // main-queue work, this block ran ~1 s late under display load (1-5 of 10 exits with a screen capture
    // running), after AppKit had given up and posted Did; the picture then stayed at the screen's size and
    // jumped 120-170 pt. From the run loop: every exit animated before Did, 200 of 200 (measured).
    CFRunLoopPerformBlock(CFRunLoopGetMain(), kCFRunLoopCommonModes, ^{
      [window setStyleMask:window.styleMask & ~NSWindowStyleMaskFullScreen];
      [window setFrame:intermediate display:NO];
      fp_animate(window, intermediate, g_windowed, duration);
    });
    CFRunLoopWakeUp(CFRunLoopGetMain());
}

// While our animation runs, the frames it asks for are taken as they are: AppKit otherwise keeps a
// titled window under the menu bar, and the first third of the way out came out shorter (measured).
static IMP g_constrain;
static NSRect fp_constrain(NSWindow *self, SEL cmd, NSRect rect, NSScreen *screen) {
    if (g_free) return rect;
    return ((NSRect(*)(id, SEL, NSRect, NSScreen *))g_constrain)(self, cmd, rect, screen);
}

// ---- The wanted state. The page asks for a state, not a toggle (`fpui_fs_want`, the stub's
// `setFullscreen` on macOS). AppKit refuses a toggle while a transition runs, so a request then waits, and the
// latest one is applied when the transition is over (Did or a failure). Before this the page went through
// Skathi's `setFullscreen`, which compares with the window's style mask, already the target's from the start
// of ours: requests mid-transition were lost and 4 of 24 quick sequences ended in the state not asked for
// (in-out-in at 60 ms windowed, in-out-in-out at 0 ms fullscreen; measured).
static NSWindow *g_window;
static BOOL g_want, g_busy, g_ours;
static void fp_apply(void);
static BOOL fp_is_full(void) { return (g_window.styleMask & NSWindowStyleMaskFullScreen) != 0; }
void fpui_fs_want(int on) {
    g_want = on != 0;
    if (!g_busy) fp_apply();
}
// What the page's mirror reads while anything runs or waits (`is_fullscreen` through the stub's `fsTarget`):
// the wanted state; -1 when idle, and the page reads the window's own. The style mask is the target's from the
// start of a transition, and the page, re-reading it on every resize, turned its next quick toggle the wrong
// way (in-out-in asked "out" twice; measured).
int fpui_fs_target(void) { return g_busy || fp_is_full() != g_want ? g_want : -1; }
static void fp_apply(void) {
    if (!g_window || g_busy || fp_is_full() == g_want) return;
    g_busy = YES;
    g_ours = YES;
    ((void (*)(id, SEL, id))g_toggle)(g_window, @selector(toggleFullScreen:), nil);
}
// A transition is over: the window's state is what it is, and a request that came meanwhile goes now (on the
// next turn of the run loop: AppKit is still finishing this one inside the notification).
static void fp_settled(void) {
    g_busy = NO;
    if (fp_is_full() != g_want) {
        CFRunLoopPerformBlock(CFRunLoopGetMain(), kCFRunLoopCommonModes, ^{ fp_apply(); });
        CFRunLoopWakeUp(CFRunLoopGetMain());
    }
}
// A transition starts: ours, or one AppKit began (the green button goes through the page and so through
// `fpui_fs_want`; leaving from Mission Control does not) whose target becomes the wanted state.
static void fp_will(BOOL entering) {
    g_busy = YES;
    if (!g_ours) g_want = entering;
    g_ours = NO;
}

static void fp_failed(NSWindow *self, SEL cmd, NSWindow *window) {
    fp_settled();
    g_free = NO;
    fp_stop();
    if (!NSIsEmptyRect(g_windowed)) [window setFrame:g_windowed display:YES];
}

void fpui_fs_install(void *nswindow, void (*request)(void), void (*step)(void), int custom) {
    NSWindow *window = (__bridge NSWindow *)nswindow;
    Class cls = [window class]; // the class itself, not the KVO subclass object_getClass would give
    g_request = request;
    g_step = step;
    g_custom = custom;
    g_window = window;
    g_want = fp_is_full();
    NSNotificationCenter *nc = NSNotificationCenter.defaultCenter;
    [nc addObserverForName:NSWindowWillEnterFullScreenNotification object:window queue:nil usingBlock:^(NSNotification *n) { fp_will(YES); }];
    [nc addObserverForName:NSWindowWillExitFullScreenNotification object:window queue:nil usingBlock:^(NSNotification *n) { fp_will(NO); }];
    [nc addObserverForName:NSWindowDidEnterFullScreenNotification object:window queue:nil usingBlock:^(NSNotification *n) { fp_settled(); }];
    [nc addObserverForName:NSWindowDidExitFullScreenNotification object:window queue:nil usingBlock:^(NSNotification *n) { fp_settled(); }];
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      g_toggle = class_getMethodImplementation([NSWindow class], @selector(toggleFullScreen:));
      class_addMethod(cls, @selector(toggleFullScreen:), (IMP)fp_toggle, "v@:@");
      class_addMethod(cls, @selector(customWindowsToEnterFullScreenForWindow:), (IMP)fp_custom_windows, "@@:@");
      class_addMethod(cls, @selector(customWindowsToExitFullScreenForWindow:), (IMP)fp_custom_exit_windows, "@@:@");
      class_addMethod(cls, @selector(window:startCustomAnimationToEnterFullScreenWithDuration:), (IMP)fp_start_enter, "v@:@d");
      class_addMethod(cls, @selector(window:startCustomAnimationToExitFullScreenWithDuration:), (IMP)fp_start_exit, "v@:@d");
      class_addMethod(cls, @selector(windowDidFailToEnterFullScreen:), (IMP)fp_failed, "v@:@");
      g_resize_imp = class_getMethodImplementation([NSWindow class], @selector(animationResizeTime:));
      class_addMethod(cls, @selector(animationResizeTime:), (IMP)fp_resize_time, method_getTypeEncoding(class_getInstanceMethod([NSWindow class], @selector(animationResizeTime:))));
      g_constrain = class_getMethodImplementation([NSWindow class], @selector(constrainFrameRect:toScreen:));
      class_addMethod(cls, @selector(constrainFrameRect:toScreen:), (IMP)fp_constrain, method_getTypeEncoding(class_getInstanceMethod([NSWindow class], @selector(constrainFrameRect:toScreen:))));
    });
}
