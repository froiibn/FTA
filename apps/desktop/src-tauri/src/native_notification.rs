//! Persistent macOS notification payloads route cold-start responses without process-local closure state.

use std::{
    ptr::NonNull,
    sync::{Arc, Mutex},
    time::Duration,
};

use block2::{DynBlock, RcBlock};
use objc2::{
    AllocAnyThread, DefinedClass, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
};
use objc2_foundation::{NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSettings,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

const TRANSFER_KEY: &str = "fta_transfer_id";
const NATIVE_COMPLETION_TIMEOUT: Duration = Duration::from_secs(10);

struct DelegateIvars {
    app: tauri::AppHandle,
}

define_class!(
    // SAFETY: NSObject has no subclass-specific invariants. Ivars contain a thread-safe AppHandle.
    #[unsafe(super(NSObject))]
    #[ivars = DelegateIvars]
    struct FtaNotificationDelegate;

    // SAFETY: NSObjectProtocol introduces no additional method requirements.
    unsafe impl NSObjectProtocol for FtaNotificationDelegate {}

    // SAFETY: Method signatures match the generated UserNotifications protocol exactly.
    unsafe impl UNUserNotificationCenterDelegate for FtaNotificationDelegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            // SAFETY: Apple's immutable exported identifier is available for the framework lifetime.
            if response
                .actionIdentifier()
                .isEqualToString(unsafe { UNNotificationDefaultActionIdentifier })
            {
                let info = response.notification().request().content().userInfo();
                let key = NSString::from_str(TRANSFER_KEY);
                if let Some(value) = info.objectForKey(&key)
                    && let Ok(id) = value.downcast::<NSString>()
                    && let Ok(id) = uuid::Uuid::parse_str(&id.to_string())
                {
                    super::queue_activation(
                        &self.ivars().app,
                        &format!("{}://inbox/{id}", super::ACTIVATION_SCHEME),
                    );
                }
            }
            completion.call(());
        }

        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }
    }
);

/// Retain the weak center delegate for the entire app lifetime, including responses delivered on startup.
pub struct NativeNotificationDelegate {
    _delegate: Retained<FtaNotificationDelegate>,
}

/// A raw development executable is not a notification-capable application bundle.
fn center() -> Result<Retained<UNUserNotificationCenter>, String> {
    let bundle = NSBundle::mainBundle();
    if bundle.bundleIdentifier().is_none() || !bundle.bundlePath().to_string().ends_with(".app") {
        return Err("macOS 알림은 설치된 FTA 앱에서 사용할 수 있습니다".into());
    }
    Ok(UNUserNotificationCenter::currentNotificationCenter())
}

/// Install before app.run: Tauri setup runs after applicationDidFinishLaunching, too late for cold-start callbacks.
pub fn install(app: &tauri::AppHandle) -> Result<NativeNotificationDelegate, String> {
    let center = center()?;
    let allocated = FtaNotificationDelegate::alloc().set_ivars(DelegateIvars { app: app.clone() });
    // SAFETY: NSObject's designated initializer is used after all Rust ivars have been initialized.
    let delegate: Retained<FtaNotificationDelegate> = unsafe { msg_send![super(allocated), init] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    Ok(NativeNotificationDelegate {
        _delegate: delegate,
    })
}

/// Query real OS authorization instead of reporting Tauri's desktop always-granted placeholder.
pub async fn permission(request: bool) -> Result<String, String> {
    let receiver = {
        let center = match center() {
            Ok(center) => center,
            Err(_) => return Ok("unavailable".into()),
        };
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));
        if request {
            let completion = RcBlock::new(move |granted: Bool, error: *mut NSError| {
                let result = if error.is_null() {
                    Ok(if granted.as_bool() {
                        "granted"
                    } else {
                        "denied"
                    }
                    .to_owned())
                } else {
                    // SAFETY: NSError is a nullable framework-owned object valid throughout this callback.
                    Err(unsafe { (&*error).localizedDescription().to_string() })
                };
                if let Ok(mut sender) = sender.lock()
                    && let Some(sender) = sender.take()
                {
                    let _ = sender.send(result);
                }
            });
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &completion,
            );
        } else {
            let completion = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
                // SAFETY: The framework supplies a non-null settings object valid for the callback.
                let status = unsafe { settings.as_ref() }.authorizationStatus();
                let status = if status == UNAuthorizationStatus::NotDetermined {
                    "prompt"
                } else if status == UNAuthorizationStatus::Denied {
                    "denied"
                } else if [
                    UNAuthorizationStatus::Authorized,
                    UNAuthorizationStatus::Provisional,
                    UNAuthorizationStatus::Ephemeral,
                ]
                .contains(&status)
                {
                    "granted"
                } else {
                    "unavailable"
                };
                if let Ok(mut sender) = sender.lock()
                    && let Some(sender) = sender.take()
                {
                    let _ = sender.send(Ok(status.to_owned()));
                }
            });
            center.getNotificationSettingsWithCompletionHandler(&completion);
        }
        receiver
    };
    tokio::time::timeout(NATIVE_COMPLETION_TIMEOUT, receiver)
        .await
        .map_err(|_| "알림 권한 응답 시간이 초과되었습니다")?
        .map_err(|_| "알림 권한 응답이 중단되었습니다")?
}

/// Persist the UUID in userInfo and await the framework's request-acceptance callback before acknowledging the event.
pub async fn show(transfer_id: &str, title: &str, body: &str) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(transfer_id).map_err(|_| "올바르지 않은 전송 ID입니다")?;
    let receiver = {
        let center = center()?;
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let key = NSString::from_str(TRANSFER_KEY);
        let value = NSString::from_str(&id.to_string());
        let info = NSDictionary::from_slices(&[&*key], &[&*value]);
        // SAFETY: Erasing NSDictionary's generic parameters is valid; both entries are immutable NSString property-list values.
        let info: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(info) };
        // SAFETY: The dictionary contains only NSString property-list values and is copied by the content object.
        unsafe {
            content.setUserInfo(&info);
        }
        let identifier = NSString::from_str(&format!("fta-{id}"));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));
        let completion = RcBlock::new(move |error: *mut NSError| {
            let result = if error.is_null() {
                Ok(())
            } else {
                // SAFETY: NSError is retained by the framework for this completion callback.
                Err(unsafe { (&*error).localizedDescription().to_string() })
            };
            if let Ok(mut sender) = sender.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(result);
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&completion));
        receiver
    };
    tokio::time::timeout(NATIVE_COMPLETION_TIMEOUT, receiver)
        .await
        .map_err(|_| "알림 등록 응답 시간이 초과되었습니다")?
        .map_err(|_| "알림 등록 응답이 중단되었습니다")?
}
