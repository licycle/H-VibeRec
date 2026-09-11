// macOS adapter. UI entry points run on the main thread; capture_origin is a
// read-only shortcut-thread snapshot and must never dispatch back to the UI.
#import <Cocoa/Cocoa.h>
#import <UserNotifications/UserNotifications.h>

typedef void (*HVRCallback)(const char *);
static HVRCallback callback;
static NSStatusItem *statusItem;
static NSString *notificationStatus = @"unavailable";
static BOOL notificationsEnabled = YES;
static NSUInteger notificationGeneration;

static char *JSONResult(id value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:nil];
    if (!data) return strdup("{\"error\":\"无法编码系统操作结果\"}");
    return strdup([[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding].UTF8String);
}
static char *Failure(NSString *message) { return JSONResult(@{@"error": message}); }
char *hvr_pastebox_capture_origin(void) {
    @autoreleasepool {
        NSRunningApplication *front = NSWorkspace.sharedWorkspace.frontmostApplication;
        NSMutableArray *input = [NSMutableArray new];
        for (NSNumber *type in @[@(kCGEventKeyDown), @(kCGEventLeftMouseDown),
                                 @(kCGEventRightMouseDown), @(kCGEventOtherMouseDown)]) {
            [input addObject:@(CGEventSourceCounterForEventType(
                kCGEventSourceStateCombinedSessionState, type.unsignedIntValue))];
        }
        return JSONResult(@{@"pid": @(front.processIdentifier), @"input": input});
    }
}
static void Emit(NSDictionary *event) {
    if (callback) {
        char *json = JSONResult(event);
        callback(json);
        free(json);
    }
}
static NSArray<NSPasteboardItem *> *ClipboardSnapshot(NSPasteboard *pb) {
    NSMutableArray *items = [NSMutableArray new];
    for (NSPasteboardItem *item in pb.pasteboardItems) {
        NSPasteboardItem *copy = [NSPasteboardItem new];
        for (NSPasteboardType type in item.types) {
            NSData *data = [item dataForType:type];
            if (data) [copy setData:data forType:type];
        }
        [items addObject:copy];
    }
    return items;
}
@interface HVRController : NSObject <UNUserNotificationCenterDelegate>
- (void)openPastebox:(id)sender;
@end
static HVRController *controller;
void hvr_pastebox_allow_target_window(void *window) {
    [(__bridge NSWindow *)window setAccessibilityIdentifier:@"hvr.pastebox.main"];
}
@implementation HVRController
- (void)openPastebox:(id)sender {
    pid_t expectedPID = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
    NSRect screenRect = [statusItem.button.window convertRectToScreen:statusItem.button.frame];
    NSRect visible = statusItem.button.window.screen.visibleFrame;
    CGFloat primaryHeight = NSScreen.screens.firstObject.frame.size.height;
    Emit(@{@"type": @"open", @"expected_pid": @(expectedPID),
        @"x": @(fmin(fmax(NSMidX(screenRect), NSMinX(visible) + 220), NSMaxX(visible) - 220)),
        @"y": @(primaryHeight - NSMinY(screenRect))});
}
- (void)userNotificationCenter:(UNUserNotificationCenter *)center willPresentNotification:(UNNotification *)notification
    withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completionHandler {
    if (!notificationsEnabled) { completionHandler(UNNotificationPresentationOptionNone); return; }
    if (@available(macOS 11.0, *)) completionHandler(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList | UNNotificationPresentationOptionSound);
    else completionHandler(UNNotificationPresentationOptionAlert | UNNotificationPresentationOptionSound);
}
- (void)userNotificationCenter:(UNUserNotificationCenter *)center didReceiveNotificationResponse:(UNNotificationResponse *)response
    withCompletionHandler:(void (^)(void))completionHandler {
    NSString *identifier = response.notification.request.identifier;
    BOOL activate = [response.actionIdentifier isEqual:UNNotificationDefaultActionIdentifier] ||
        [response.actionIdentifier isEqual:@"hvr.paste"] || [response.actionIdentifier isEqual:@"hvr.restore"];
    completionHandler();
    if (activate) dispatch_async(dispatch_get_main_queue(), ^{ Emit(@{@"type": @"notification", @"id": identifier}); });
}
@end

static void RefreshNotificationStatus(void) {
    if ([notificationStatus isEqual:@"unavailable"]) return;
    [UNUserNotificationCenter.currentNotificationCenter getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
        dispatch_async(dispatch_get_main_queue(), ^{
            NSString *previous = notificationStatus;
            switch (settings.authorizationStatus) {
                case UNAuthorizationStatusAuthorized: case UNAuthorizationStatusProvisional: notificationStatus = @"granted"; break;
                case UNAuthorizationStatusDenied: notificationStatus = @"denied"; break;
                default: notificationStatus = @"not_determined"; break;
            }
            if (![previous isEqual:notificationStatus]) Emit(@{@"type": @"changed"});
        });
    }];
}

void hvr_pastebox_init(HVRCallback handler) {
    callback = handler;
    controller = [HVRController new];
    statusItem = [NSStatusBar.systemStatusBar statusItemWithLength:NSVariableStatusItemLength];
    statusItem.button.title = @"粘贴";
    statusItem.button.toolTip = @"打开语音粘贴箱";
    statusItem.button.target = controller;
    statusItem.button.action = @selector(openPastebox:);
    [statusItem.button sendActionOn:NSEventMaskLeftMouseDown];
    if (NSBundle.mainBundle.bundleIdentifier.length && [NSBundle.mainBundle.bundleURL.pathExtension isEqual:@"app"]) {
        notificationStatus = @"not_determined";
        UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
        center.delegate = controller;
        UNNotificationAction *paste = [UNNotificationAction actionWithIdentifier:@"hvr.paste" title:@"粘贴到记录位置" options:UNNotificationActionOptionNone];
        UNNotificationAction *restore = [UNNotificationAction actionWithIdentifier:@"hvr.restore" title:@"返回对应位置" options:UNNotificationActionOptionNone];
        UNNotificationCategory *pasteCategory = [UNNotificationCategory categoryWithIdentifier:@"hvr.dictation.paste" actions:@[paste] intentIdentifiers:@[] options:UNNotificationCategoryOptionNone];
        UNNotificationCategory *restoreCategory = [UNNotificationCategory categoryWithIdentifier:@"hvr.dictation.restore" actions:@[restore] intentIdentifiers:@[] options:UNNotificationCategoryOptionNone];
        [center setNotificationCategories:[NSSet setWithObjects:pasteCategory, restoreCategory, nil]];
        RefreshNotificationStatus();
    }
}

char *hvr_pastebox_call(const char *input) {
    @autoreleasepool {
        NSDictionary *args = [NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:input] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
        NSString *op = args[@"op"];
        if ([op isEqual:@"frontmost"]) return JSONResult(@{@"pid": @(NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier)});
        if ([op isEqual:@"copy"]) {
            NSPasteboard *pb = NSPasteboard.generalPasteboard;
            NSArray *previous = ClipboardSnapshot(pb);
            [pb clearContents];
            if (![pb setString:args[@"text"] forType:NSPasteboardTypeString]) {
                [pb clearContents]; if (previous.count) [pb writeObjects:previous]; return Failure(@"无法复制文字");
            }
            return JSONResult(@{});
        }
        if ([op isEqual:@"status"]) {
            RefreshNotificationStatus();
            return JSONResult(@{@"notification_status": notificationStatus});
        }
        if ([op isEqual:@"count"]) {
            NSInteger count = [args[@"count"] integerValue];
            statusItem.button.title = count > 0 ? [NSString stringWithFormat:@"粘贴 %ld", (long)count] : @"粘贴";
            return JSONResult(@{});
        }
        if ([op isEqual:@"permission"]) {
            if ([notificationStatus isEqual:@"unavailable"]) return Failure(@"系统通知需要从已打包的 H-VibeRec.app 中启用");
            [UNUserNotificationCenter.currentNotificationCenter requestAuthorizationWithOptions:(UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge) completionHandler:^(BOOL granted, NSError *error) { RefreshNotificationStatus(); }];
            return JSONResult(@{});
        }
        if ([op isEqual:@"notify"]) {
            if (!notificationsEnabled) return Failure(@"完成通知已关闭");
            if ([notificationStatus isEqual:@"unavailable"]) return Failure(@"系统通知需要从 H-VibeRec.app 中发送");
            NSUInteger generation = notificationGeneration;
            BOOL restore = [args[@"action"] isEqual:@"restore"];
            UNMutableNotificationContent *content = [UNMutableNotificationContent new];
            content.title = [NSString stringWithFormat:@"#%@ %@", args[@"seq"], args[@"title"]];
            content.subtitle = [NSString stringWithFormat:@"点击复制结果并返回 %@ 的对应位置", args[@"target_name"]];
            content.body = args[@"preview"];
            content.categoryIdentifier = restore ? @"hvr.dictation.restore" : @"hvr.dictation.paste";
            content.userInfo = @{@"item_id": args[@"id"], @"action": restore ? @"restore" : @"paste"};
            UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:args[@"id"] content:content trigger:nil];
            [UNUserNotificationCenter.currentNotificationCenter getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
                dispatch_async(dispatch_get_main_queue(), ^{
                if (!notificationsEnabled || generation != notificationGeneration) {
                    Emit(@{@"type":@"notification_result", @"id":args[@"id"], @"error":@"完成通知已关闭"});
                    return;
                }
                if (settings.authorizationStatus != UNAuthorizationStatusAuthorized && settings.authorizationStatus != UNAuthorizationStatusProvisional) {
                    dispatch_async(dispatch_get_main_queue(), ^{
                        Emit(@{@"type": @"notification_result", @"id": args[@"id"], @"error": @"系统通知未获允许，请在系统设置中启用 H-VibeRec 通知；本条内容已保留"});
                    });
                    return;
                }
                [UNUserNotificationCenter.currentNotificationCenter addNotificationRequest:request withCompletionHandler:^(NSError *error) {
                    dispatch_async(dispatch_get_main_queue(), ^{
                        if (!notificationsEnabled || generation != notificationGeneration) {
                            [UNUserNotificationCenter.currentNotificationCenter removePendingNotificationRequestsWithIdentifiers:@[args[@"id"]]];
                            [UNUserNotificationCenter.currentNotificationCenter removeDeliveredNotificationsWithIdentifiers:@[args[@"id"]]];
                        }
                        Emit(@{@"type": @"notification_result", @"id": args[@"id"], @"error": error ? error.localizedDescription : NSNull.null});
                    });
                }];
                });
            }];
            return JSONResult(@{});
        }
        if ([op isEqual:@"notifications_enabled"]) {
            notificationsEnabled = [args[@"enabled"] boolValue];
            notificationGeneration++;
            if (!notificationsEnabled && ![notificationStatus isEqual:@"unavailable"]) {
                [UNUserNotificationCenter.currentNotificationCenter removeAllPendingNotificationRequests];
                [UNUserNotificationCenter.currentNotificationCenter removeAllDeliveredNotifications];
            }
            return JSONResult(@{});
        }
        if ([op isEqual:@"remove_notification"]) {
            if (![notificationStatus isEqual:@"unavailable"]) {
                [UNUserNotificationCenter.currentNotificationCenter removePendingNotificationRequestsWithIdentifiers:@[args[@"id"]]];
                [UNUserNotificationCenter.currentNotificationCenter removeDeliveredNotificationsWithIdentifiers:@[args[@"id"]]];
            }
            return JSONResult(@{});
        }
        return Failure(@"未知的系统操作");
    }
}
void hvr_pastebox_free(char *value) { free(value); }
