use std::cell::RefCell;
use std::os::raw::c_void;
use std::ptr;
use std::sync::{Mutex, OnceLock};
use tiny_http::{Header, Request, Response, Server};

extern "C" {
    fn light_str_from_utf8(ptr: *const u8, len: usize) -> *mut c_void;
    fn light_str_ptr(s: *const c_void) -> *const u8;
    fn light_str_len(s: *const c_void) -> usize;
    fn light_str_free(s: *mut c_void);
}

pub type LightHandler = unsafe extern "C" fn(i64) -> i64;

#[derive(Clone, Copy)]
enum RouteMethod {
    Get,
    Post,
}

struct Route {
    method: RouteMethod,
    path: String,
    handler: LightHandler,
    html: bool,
}

struct WebRequest {
    method: String,
    path: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

struct PageState {
    title: String,
    icon: String,
    lang: String,
    theme: String,
    styles: Vec<u8>,
    content: Vec<u8>,
}

thread_local! {
    static PAGE: RefCell<PageState> = RefCell::new(PageState {
        title: String::new(),
        icon: String::new(),
        lang: String::from("zh-cn"),
        theme: String::from("light"),
        styles: Vec::new(),
        content: Vec::new(),
    });
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn string_from_ptr(ptr: *const u8, len: i64) -> String {
    if ptr.is_null() || len < 0 {
        return String::new();
    }
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len as usize) }).to_string()
}

fn page_append_style(value: &str) {
    PAGE.with(|page| page.borrow_mut().styles.extend_from_slice(value.as_bytes()));
}

fn page_append_str(value: &str) {
    PAGE.with(|page| page.borrow_mut().content.extend_from_slice(value.as_bytes()));
}

fn page_append_value(value: impl std::fmt::Display) {
    let text = value.to_string();
    PAGE.with(|page| page.borrow_mut().content.extend_from_slice(text.as_bytes()));
}

fn routes() -> &'static Mutex<Vec<Route>> {
    static ROUTES: OnceLock<Mutex<Vec<Route>>> = OnceLock::new();
    ROUTES.get_or_init(|| Mutex::new(Vec::new()))
}

unsafe fn make_string(value: &[u8]) -> i64 {
    light_str_from_utf8(value.as_ptr(), value.len()) as i64
}

unsafe fn light_string_bytes<'a>(value: i64) -> Option<&'a [u8]> {
    if value == 0 {
        return None;
    }
    let ptr = value as *const c_void;
    Some(std::slice::from_raw_parts(
        light_str_ptr(ptr),
        light_str_len(ptr),
    ))
}

unsafe fn register_route(
    method: i32,
    path_ptr: *const u8,
    path_len: i64,
    handler: LightHandler,
    html: bool,
) -> i64 {
    if path_ptr.is_null() || path_len < 0 {
        return -1;
    }
    let method = match method {
        0 => RouteMethod::Get,
        1 => RouteMethod::Post,
        _ => return -1,
    };
    let path = match std::str::from_utf8(std::slice::from_raw_parts(path_ptr, path_len as usize)) {
        Ok(path) => path.to_string(),
        Err(_) => return -1,
    };
    if !path.starts_with('/') {
        return -1;
    }
    let mut list = match routes().lock() {
        Ok(list) => list,
        Err(_) => return -1,
    };
    if list
        .iter()
        .any(|route| route.method as i32 == method as i32 && route.path == path)
    {
        return -2;
    }
    list.push(Route { method, path, handler, html });
    0
}

#[no_mangle]
pub unsafe extern "C" fn light_web_register_v1(
    method: i32,
    path_ptr: *const u8,
    path_len: i64,
    handler: LightHandler,
) -> i64 {
    register_route(method, path_ptr, path_len, handler, false)
}

#[no_mangle]
pub unsafe extern "C" fn light_web_register_html_v1(
    method: i32,
    path_ptr: *const u8,
    path_len: i64,
    handler: LightHandler,
) -> i64 {
    register_route(method, path_ptr, path_len, handler, true)
}

#[no_mangle]
pub unsafe extern "C" fn light_page_reset(
    title_ptr: *const u8,
    title_len: i64,
    icon_ptr: *const u8,
    icon_len: i64,
) {
    let title = string_from_ptr(title_ptr, title_len);
    let icon = string_from_ptr(icon_ptr, icon_len);
    PAGE.with(|page| {
        *page.borrow_mut() = PageState {
            title,
            icon,
            lang: String::from("zh-cn"),
            theme: String::from("light"),
            styles: Vec::new(),
            content: Vec::new(),
        };
    });
}

#[no_mangle]
pub unsafe extern "C" fn light_page_set_lang(ptr: *const u8, len: i64) {
    let value = string_from_ptr(ptr, len);
    PAGE.with(|page| page.borrow_mut().lang = value);
}

#[no_mangle]
pub unsafe extern "C" fn light_page_set_theme(ptr: *const u8, len: i64) {
    let value = string_from_ptr(ptr, len);
    PAGE.with(|page| page.borrow_mut().theme = value);
}

#[no_mangle]
pub unsafe extern "C" fn light_page_append_style(ptr: *const u8, len: i64) {
    page_append_style(&string_from_ptr(ptr, len));
}

#[no_mangle]
pub unsafe extern "C" fn light_page_append_str(ptr: *const u8, len: i64) {
    page_append_str(&string_from_ptr(ptr, len));
}

#[no_mangle]
pub unsafe extern "C" fn light_page_append_int(value: i64) {
    page_append_value(value);
}

#[no_mangle]
pub unsafe extern "C" fn light_page_append_float(value: f64) {
    page_append_value(value);
}

#[no_mangle]
pub unsafe extern "C" fn light_page_append_bool(value: i64) {
    page_append_value(value != 0);
}

#[no_mangle]
pub unsafe extern "C" fn light_page_finish() -> *mut c_void {
    PAGE.with(|page| {
        let mut page = page.borrow_mut();
        let title = html_escape(&page.title);
        let lang = html_escape(&page.lang);
        let icon = html_escape(&page.icon);
        let content = String::from_utf8_lossy(&page.content).to_string();
        let styles = String::from_utf8_lossy(&page.styles).to_string();
        let (background, foreground) = if page.theme == "dark" {
            ("#101418", "#e6edf3")
        } else {
            ("#ffffff", "#111827")
        };
        let icon_link = if page.icon.is_empty() || page.icon == "none" {
            String::new()
        } else {
            format!("<link rel=\"icon\" href=\"{}\">\n", icon)
        };
        let html = format!(
            "<!doctype html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n{icon_link}<style>\nbody {{ margin: 0; padding: 2rem; background: {background}; color: {foreground}; font-family: system-ui, sans-serif; }}\n{styles}\n</style>\n</head>\n<body>\n{content}\n</body>\n</html>\n"
        );
        page.content.clear();
        page.styles.clear();
        make_string(html.as_bytes()) as *mut c_void
    })
}

#[no_mangle]
pub unsafe extern "C" fn light_web_request_method_v1(request: i64) -> i64 {
    let request = &*(request as *const WebRequest);
    make_string(request.method.as_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn light_web_request_path_v1(request: i64) -> i64 {
    let request = &*(request as *const WebRequest);
    make_string(request.path.as_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn light_web_request_body_v1(request: i64) -> i64 {
    let request = &*(request as *const WebRequest);
    make_string(&request.body)
}

#[no_mangle]
pub unsafe extern "C" fn light_web_request_header_v1(
    request: i64,
    name_ptr: *const u8,
    name_len: i64,
) -> i64 {
    let request = &*(request as *const WebRequest);
    if name_ptr.is_null() || name_len < 0 {
        return make_string(&[]);
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(
        name_ptr,
        name_len as usize,
    )) {
        Ok(name) => name.to_ascii_lowercase(),
        Err(_) => return make_string(&[]),
    };
    for (key, value) in &request.headers {
        if key.to_ascii_lowercase() == name {
            return make_string(value.as_bytes());
        }
    }
    make_string(&[])
}

fn respond_error(request: Request, status: u16, message: &str) {
    let response = Response::from_string(message.to_string()).with_status_code(status);
    let _ = request.respond(response);
}

fn handle_request(mut request: Request) {
    let method = request.method().as_str().to_string();
    let path = request.url().split('?').next().unwrap_or("/").to_string();
    let mut body = Vec::new();
    if request.as_reader().read_to_end(&mut body).is_err() {
        respond_error(request, 400, "读取请求失败");
        return;
    }
    let headers = request
        .headers()
        .iter()
        .map(|header| {
            (
                header.field.as_str().as_str().to_ascii_lowercase(),
                header.value.as_str().to_string(),
            )
        })
        .collect();
    let route = {
        let list = match routes().lock() {
            Ok(list) => list,
            Err(_) => {
                respond_error(request, 500, "路由表锁定失败");
                return;
            }
        };
        list.iter()
            .find(|route| {
                let route_method = match route.method {
                    RouteMethod::Get => "GET",
                    RouteMethod::Post => "POST",
                };
                route_method == method && route.path == path
            })
            .map(|route| (route.handler, route.html))
    };
    let (handler, html_response) = match route {
        Some(route) => route,
        None => {
            let allowed = {
                let list = routes().lock().ok();
                list.map(|list| {
                    list.iter().any(|route| {
                        let route_method = match route.method {
                            RouteMethod::Get => "GET",
                            RouteMethod::Post => "POST",
                        };
                        route_method == method && route.path == path
                    })
                })
            };
            if allowed == Some(true) {
                respond_error(request, 405, "请求方法不允许");
            } else {
                respond_error(request, 404, "页面不存在");
            }
            return;
        }
    };
    let web_request = WebRequest {
        method,
        path,
        body,
        headers,
    };
    let response_value = unsafe { handler(&web_request as *const WebRequest as i64) };
    let response_text = match unsafe { light_string_bytes(response_value) } {
        Some(bytes) => String::from_utf8_lossy(bytes).to_string(),
        None => {
            respond_error(request, 500, "处理器没有返回字符串");
            return;
        }
    };
    unsafe { light_str_free(response_value as *mut c_void) };
    let content_type_value: &[u8] = if html_response {
        b"text/html; charset=utf-8"
    } else {
        b"text/plain; charset=utf-8"
    };
    let content_type = Header::from_bytes(&b"Content-Type"[..], content_type_value);
    let response = Response::from_string(response_text).with_status_code(200);
    let response = match content_type {
        Ok(header) => response.with_header(header),
        Err(_) => response,
    };
    let _ = request.respond(response);
}

#[no_mangle]
pub unsafe extern "C" fn light_web_run_v1(port: i64) -> i64 {
    let port = if port == 0 {
        std::env::var("LIGHT_WEB_PORT")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(8080)
    } else {
        port
    };
    if !(1..=65535).contains(&port) {
        return -1;
    }
    let _ = std::hint::black_box(lightrt::str_parts(ptr::null()));
    let address = format!("127.0.0.1:{}", port);
    let server = match Server::http(address.as_str()) {
        Ok(server) => server,
        Err(_) => return -1,
    };
    for request in server.incoming_requests() {
        handle_request(request);
    }
    0
}
