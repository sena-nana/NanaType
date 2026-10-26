//! Finder Open With and document Apple Events use the same unsaved flow as CLI.
use crate::msg::Msg;
use nana_ui::RuntimeProgramContext;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSApplicationTerminateReply};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NanaTypeEditorDocumentDelegate"]
    #[ivars = RuntimeProgramContext<Msg>]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _: &NSApplication) -> NSApplicationTerminateReply {
            self.ivars().dispatch(Msg::PackageEditor(crate::session::Action::Exit));
            NSApplicationTerminateReply::TerminateCancel
        }
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _: &NSApplication, urls: &NSArray<NSURL>) {
            self.ivars().dispatch(Msg::SecondInstance);
            for url in urls {
                if url.isFileURL() && let Some(path) = url.path() {
                    self.ivars().dispatch(Msg::Open(path.to_string().into()));
                }
            }
        }
    }
);

pub fn install(context: &RuntimeProgramContext<Msg>) -> Result<Retained<Delegate>, String> {
    let mtm = MainThreadMarker::new().ok_or("document events require the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    // Winit deliberately leaves the application delegate to its host. Keep the
    // delegate alive in Host because AppKit's delegate reference is weak.
    if app.delegate().is_some() {
        return Err("an application document delegate is already registered".into());
    }
    let delegate: Retained<Delegate> =
        unsafe { msg_send![super(Delegate::alloc(mtm).set_ivars(context.clone())), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    Ok(delegate)
}
