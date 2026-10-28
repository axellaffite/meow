//! Embedded Slack sign-in.
//!
//! A plain window with a `WKWebView` pointed at Slack's own web app. The user
//! signs in there with their usual SSO; Slack cannot tell the view apart from a
//! browser. Once signed in, Meow reads the two things the web client itself
//! uses — the session token from `localStorage` and the `d` cookie from the
//! view's cookie store — and hands them back. Nothing is ever typed or pasted.
//!
//! Both reads are asynchronous, so [`SignIn::poll`] is called from the app tick:
//! it kicks the reads off every second and resolves once both have arrived.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{
    NSArray, NSBundle, NSError, NSHTTPCookie, NSPoint, NSRect, NSSize, NSString, NSURL,
    NSURLRequest,
};
use objc2_web_kit::{
    WKNavigationAction, WKUIDelegate, WKWebView, WKWebViewConfiguration, WKWindowFeatures,
};

use meow::slack::auth::{self, Credentials};

/// Slack's sign-in entry: pick a workspace, sign in with SSO, and land in the
/// web client, which then holds both secrets. (`app.slack.com` with no session
/// bounces to the marketing site instead.)
const SIGN_IN_URL: &str = "https://slack.com/signin";
/// Probe for the token and cookie about once a second (the tick runs at 30 Hz).
const PROBE_EVERY_TICKS: u32 = 30;
const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 640.0;
/// Slack rejects unknown browsers on its sign-in page, and WKWebView's default
/// user agent lacks Safari's `Version/… Safari/…` tokens. The view presents a
/// desktop Safari user agent: it matches the WebKit engine underneath, so
/// Slack's and SSO providers' feature checks agree with it (a Chrome user agent
/// on WebKit is flagged as an unsupported or unsafe browser). Slack only accepts
/// recent Safari versions, so the version is the installed Safari's, which is
/// also the system WebKit's.
const USER_AGENT_PREFIX: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/605.1.15 (KHTML, like Gecko)";
const SAFARI_PATH: &str = "/Applications/Safari.app";
/// Used when Safari's version cannot be read.
const FALLBACK_SAFARI_VERSION: &str = "26.0";
/// Reads the token store, plus a hint for which workspace is active: the team id
/// from the client URL (`/client/T…`), else the host's subdomain.
const READ_SCRIPT: &str = "JSON.stringify({c: localStorage.getItem('localConfig_v2'), \
    t: (location.pathname.match(/\\/client\\/(T[A-Z0-9]+)/) || [])[1] || location.host.split('.')[0], \
    u: navigator.userAgent, p: location.host + location.pathname})";

define_class!(
    /// Opens links that ask for a new window (Slack's workspace SSO button)
    /// in the sign-in view itself: a bare web view ignores them.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MeowSignInUIDelegate"]
    pub(super) struct NewWindowsInPlace;

    unsafe impl NSObjectProtocol for NewWindowsInPlace {}

    unsafe impl WKUIDelegate for NewWindowsInPlace {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            navigation_action: &WKNavigationAction,
            _window_features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            // SAFETY: WebKit calls this on the main thread with a live action and view.
            unsafe { web_view.loadRequest(&navigation_action.request()) };
            None
        }
    }
);

impl NewWindowsInPlace {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's initializer, called once on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }
}

/// An in-progress sign-in window.
pub struct SignIn {
    window: Retained<NSWindow>,
    webview: Retained<WKWebView>,
    /// The view's UI delegate, which the view does not retain.
    _ui_delegate: Retained<NewWindowsInPlace>,
    /// The `localConfig_v2` value, once read.
    config_json: Rc<RefCell<Option<String>>>,
    /// Which workspace is active (team id or subdomain), to pick the right one.
    hint: Rc<RefCell<Option<String>>>,
    /// Guards the one-time log of the user agent the page sees.
    logged_ua: Rc<RefCell<bool>>,
    /// The `d` cookie value, once read.
    cookie: Rc<RefCell<Option<String>>>,
    /// Where the page is (host and path, no query), for the progress log.
    page: Rc<RefCell<String>>,
    /// The last progress line logged, so each change is logged once.
    reported: RefCell<String>,
    ticks: u32,
}

impl SignIn {
    /// Opens the sign-in window and starts loading Slack.
    pub fn present(mtm: MainThreadMarker) -> Self {
        let frame = rect(0.0, 0.0, WIDTH, HEIGHT);
        // SAFETY: a default configuration created on the main thread.
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        // SAFETY: a fresh web view with a fresh configuration, on the main thread.
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        // SAFETY: a plain string property; setting it before any load takes effect.
        unsafe { webview.setCustomUserAgent(Some(&NSString::from_str(&user_agent()))) };
        let ui_delegate = NewWindowsInPlace::new(mtm);
        // SAFETY: the delegate is kept alive in `SignIn` as long as the view.
        unsafe { webview.setUIDelegate(Some(ProtocolObject::from_ref(&*ui_delegate))) };

        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::Miniaturizable;
        // SAFETY: standard window initializer with a valid style and backing store.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: a Retained keeps the window alive; closing must not free it.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("Sign in to Slack"));
        window.setContentView(Some(&webview));
        window.center();
        window.makeKeyAndOrderFront(None);

        if let Some(url) = NSURL::URLWithString(&NSString::from_str(SIGN_IN_URL)) {
            let request = NSURLRequest::requestWithURL(&url);
            // SAFETY: a request built from a valid URL, loaded on the main thread.
            unsafe { webview.loadRequest(&request) };
        }

        Self {
            window,
            webview,
            _ui_delegate: ui_delegate,
            config_json: Rc::new(RefCell::new(None)),
            hint: Rc::new(RefCell::new(None)),
            logged_ua: Rc::new(RefCell::new(false)),
            cookie: Rc::new(RefCell::new(None)),
            page: Rc::new(RefCell::new(String::new())),
            reported: RefCell::new(String::new()),
            ticks: 0,
        }
    }

    /// Called each tick: returns the credentials once sign-in has produced both
    /// the token and the cookie, otherwise `None`.
    pub fn poll(&mut self) -> Option<Credentials> {
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks.is_multiple_of(PROBE_EVERY_TICKS) {
            self.read_config();
            self.read_cookie();
            self.report();
        }
        self.try_build()
    }

    /// Logs how far sign-in has got whenever that changes: the page, and
    /// whether the token store and cookie were found (never their values).
    fn report(&self) {
        let json = self.config_json.borrow().clone();
        let workspaces = json
            .as_deref()
            .map(|json| auth::parse_local_config(json).map(|list| list.len()));
        let workspaces = match workspaces {
            None => "no token store".to_string(),
            Some(Ok(count)) => format!("{count} workspace(s) in the token store"),
            Some(Err(error)) => format!("token store unreadable: {error}"),
        };
        let cookie = if self.cookie.borrow().is_some() {
            "d cookie found"
        } else {
            "no d cookie"
        };
        let line = format!("page {}, {workspaces}, {cookie}", self.page.borrow());
        if *self.reported.borrow() != line {
            eprintln!("meow: slack sign-in: {line}");
            self.reported.replace(line);
        }
    }

    /// Whether the window is still open; a user who closes it abandons sign-in.
    pub fn is_open(&self) -> bool {
        self.window.isVisible()
    }

    pub fn close(&self) {
        self.window.close();
    }

    /// Assembles the credentials if both halves have arrived and look valid.
    fn try_build(&self) -> Option<Credentials> {
        let json = self.config_json.borrow().clone()?;
        let cookie = self.cookie.borrow().clone()?;
        let workspaces = auth::parse_local_config(&json).ok()?;
        let hint = self.hint.borrow().clone();
        let Some(workspace) = auth::select_workspace(&workspaces, hint.as_deref()) else {
            self.report_once(&format!("no workspace matches the page ({hint:?})"));
            return None;
        };
        let credentials = workspace.with_cookie(cookie);
        if !credentials.looks_valid() {
            self.report_once("the token or cookie does not look like Slack's");
            return None;
        }
        Some(credentials)
    }

    fn report_once(&self, line: &str) {
        if *self.reported.borrow() != line {
            eprintln!("meow: slack sign-in: {line}");
            self.reported.replace(line.to_string());
        }
    }

    /// Reads `localConfig_v2` and the host from the page.
    fn read_config(&self) {
        let config_json = Rc::clone(&self.config_json);
        let hint = Rc::clone(&self.hint);
        let logged_ua = Rc::clone(&self.logged_ua);
        let page = Rc::clone(&self.page);
        let handler = RcBlock::new(move |result: *mut AnyObject, _error: *mut NSError| {
            if result.is_null() {
                return;
            }
            // SAFETY: the script returns a string, bridged to an NSString.
            let Some(value) = (unsafe { Retained::retain(result.cast::<NSString>()) }) else {
                return;
            };
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&value.to_string()) else {
                return;
            };
            if let Some(u) = parsed.get("u").and_then(|v| v.as_str())
                && !logged_ua.replace(true)
            {
                eprintln!("meow: slack sign-in browser is: {u}");
            }
            if let Some(p) = parsed.get("p").and_then(|v| v.as_str()) {
                page.replace(p.to_string());
            }
            if let Some(t) = parsed.get("t").and_then(|v| v.as_str()) {
                hint.replace(Some(t.to_string()));
            }
            if let Some(c) = parsed.get("c").and_then(|v| v.as_str()) {
                config_json.replace(Some(c.to_string()));
            }
        });
        // SAFETY: the web view runs the script and calls the handler on the main thread.
        unsafe {
            self.webview.evaluateJavaScript_completionHandler(
                &NSString::from_str(READ_SCRIPT),
                Some(&handler),
            )
        };
    }

    /// Reads the `d` cookie from the view's cookie store.
    fn read_cookie(&self) {
        let cookie = Rc::clone(&self.cookie);
        // SAFETY: the configuration and its cookie store belong to the web view.
        let store = unsafe {
            self.webview
                .configuration()
                .websiteDataStore()
                .httpCookieStore()
        };
        let handler = RcBlock::new(move |cookies: NonNull<NSArray<NSHTTPCookie>>| {
            // SAFETY: WebKit hands us a valid, non-null array for the call's duration.
            let cookies = unsafe { cookies.as_ref() };
            for index in 0..cookies.count() {
                let item = cookies.objectAtIndex(index);
                let (name, value) = (item.name(), item.value());
                if name.to_string() == "d" {
                    cookie.replace(Some(value.to_string()));
                }
            }
        });
        // SAFETY: the store invokes the handler on the main thread.
        unsafe { store.getAllCookies(&handler) };
    }
}

fn user_agent() -> String {
    let version = installed_safari_version();
    let version = version.as_deref().unwrap_or(FALLBACK_SAFARI_VERSION);
    format!("{USER_AGENT_PREFIX} Version/{version} Safari/605.1.15")
}

/// The installed Safari's short version string (e.g. `27.0`), if readable.
fn installed_safari_version() -> Option<String> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(SAFARI_PATH))?;
    let value =
        bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))?;
    let version = value.downcast::<NSString>().ok()?.to_string();
    (!version.is_empty()).then_some(version)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
