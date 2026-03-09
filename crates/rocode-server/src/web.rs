use axum::http::header;
use axum::response::{Html, IntoResponse};

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

pub async fn app_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, HeaderValueStatic::CSS_UTF8),
            (header::CACHE_CONTROL, HeaderValueStatic::NO_STORE),
        ],
        include_str!("../web/app.css"),
    )
}

pub async fn app_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, HeaderValueStatic::JS_UTF8),
            (header::CACHE_CONTROL, HeaderValueStatic::NO_STORE),
        ],
        include_str!("../web/app.js"),
    )
}

struct HeaderValueStatic;

impl HeaderValueStatic {
    const CSS_UTF8: &'static str = "text/css; charset=utf-8";
    const JS_UTF8: &'static str = "application/javascript; charset=utf-8";
    const NO_STORE: &'static str = "no-store";
}
