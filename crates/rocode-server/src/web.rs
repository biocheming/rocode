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
        include_str!("../web/styles/app.css"),
    )
}

/// Concatenated JS modules — loaded order matters (globals first, bootstrap last).
const APP_JS: &str = concat!(
    include_str!("../web/js/constants.js"),
    "\n",
    include_str!("../web/js/utils.js"),
    "\n",
    include_str!("../web/js/session-data.js"),
    "\n",
    include_str!("../web/js/runtime-state.js"),
    "\n",
    include_str!("../web/js/execution-panel.js"),
    "\n",
    include_str!("../web/js/message-render.js"),
    "\n",
    include_str!("../web/js/scheduler-stage.js"),
    "\n",
    include_str!("../web/js/question-panel.js"),
    "\n",
    include_str!("../web/js/output-blocks.js"),
    "\n",
    include_str!("../web/js/sidebar.js"),
    "\n",
    include_str!("../web/js/settings.js"),
    "\n",
    include_str!("../web/js/session-actions.js"),
    "\n",
    include_str!("../web/js/commands.js"),
    "\n",
    include_str!("../web/js/streaming.js"),
    "\n",
    include_str!("../web/js/bootstrap.js"),
);

pub async fn app_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, HeaderValueStatic::JS_UTF8),
            (header::CACHE_CONTROL, HeaderValueStatic::NO_STORE),
        ],
        APP_JS,
    )
}

struct HeaderValueStatic;

impl HeaderValueStatic {
    const CSS_UTF8: &'static str = "text/css; charset=utf-8";
    const JS_UTF8: &'static str = "application/javascript; charset=utf-8";
    const NO_STORE: &'static str = "no-store";
}
