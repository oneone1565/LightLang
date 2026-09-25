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
}

struct WebRequest {
    method: String,
    path: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
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

#[no_mangle]
pub unsafe extern "C" fn light_web_register_v1(
    method: i32,
    path_ptr: *const u8,
    path_len: i64,
    handler: LightHandler,
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
    list.push(Route { method, path, handler });
    0
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
            .map(|route| route.handler)
    };
    let handler = match route {
        Some(handler) => handler,
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
    let content_type = Header::from_bytes(
        &b"Content-Type"[..],
        &b"text/plain; charset=utf-8"[..],
    );
    let response = Response::from_string(response_text).with_status_code(200);
    let response = match content_type {
        Ok(header) => response.with_header(header),
        Err(_) => response,
    };
    let _ = request.respond(response);
}

#[no_mangle]
pub unsafe extern "C" fn light_web_run_v1(port: i64) -> i64 {
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
