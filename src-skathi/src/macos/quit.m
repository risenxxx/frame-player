// Quitting stops the picture and the sound first (quit.rs).
//
// - `applicationShouldTerminate:` (Cmd+Q, the app menu's Quit, the Dock's Quit, logging out): gpui's
//   delegate answers TerminateLater and runs the app's quit hooks; ours runs before it.
// - `NSWindowWillCloseNotification` for the main window (`GPUIWindow`): the page's own close.
#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>

static void (*g_halt)(void);
static IMP g_should_terminate;

// A quit the system asks for as it logs out, restarts or shuts down (the quit event's reason).
static BOOL fp_system_quit(void) {
    NSAppleEventDescriptor *ev = [[NSAppleEventManager sharedAppleEventManager] currentAppleEvent];
    if (!ev || ev.eventClass != kCoreEventClass || ev.eventID != kAEQuitApplication) return NO;
    NSAppleEventDescriptor *why = [ev attributeDescriptorForKeyword:kAEQuitReason];
    return why != nil;
}

// The reply ends AppKit's wait inside `terminate:`; gpui has stopped the app loop by then, but its wake-up event
// can be taken by that wait. A Dock quit (an Apple Event) runs `terminate:` inside nextEventMatchingMask:,
// which then slept on with the stop flag set: 4 of 10 Dock quits hit the deadline even with the reply
// repeated (measured). So after the reply, a wake-up event every 100 ms until the process ends.
static void fp_wake(int n) {
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 100 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
      NSEvent *e = [NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0
                                     timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0];
      [NSApp postEvent:e atStart:NO];
      if (n < 30) fp_wake(n + 1);
    });
}

static void fp_reply(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
      [NSApp replyToApplicationShouldTerminate:NO];
      fp_wake(0);
    });
}

static NSApplicationTerminateReply fp_should_terminate(id self, SEL cmd, NSApplication *app) {
    if (g_halt) g_halt();
    NSApplicationTerminateReply reply = ((NSApplicationTerminateReply(*)(id, SEL, NSApplication *))g_should_terminate)(self, cmd, app);
    // gpui answers TerminateLater and never replies: AppKit then waits inside `terminate:` and the app
    // loop gpui stopped cannot return, so the process ended only at Skathi's 4 s deadline, mpv still
    // playing (sampled). gpui's quit is queued on the main queue; this reply comes after it
    // and ends AppKit's wait: NO, because the embedder ends the process (app.rs), not AppKit.
    // A logout/restart/shutdown keeps gpui's answer: a NO there would read as refusing it.
    if (reply == NSTerminateLater && !fp_system_quit()) fp_reply();
    return reply;
}

// 1 when installed, 0 when the delegate has no such method yet (the caller tries again).
int fpui_quit_install(void (*halt)(void)) {
    id delegate = NSApp.delegate;
    if (!delegate) return 0;
    Method m = class_getInstanceMethod([delegate class], @selector(applicationShouldTerminate:));
    if (!m) return 0;
    g_halt = halt;
    g_should_terminate = method_setImplementation(m, (IMP)fp_should_terminate);
    [[NSNotificationCenter defaultCenter]
        addObserverForName:NSWindowWillCloseNotification
                    object:nil
                     queue:nil
                usingBlock:^(NSNotification *note) {
                  NSString *name = NSStringFromClass([note.object class]);
                  if ([name hasSuffix:@"GPUIWindow"] && g_halt) g_halt();
                }];
    return 1;
}
