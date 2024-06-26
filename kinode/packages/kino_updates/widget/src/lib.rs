use kinode_process_lib::{call_init, http, timer, Address, Request};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

/// 20 minutes
const REFRESH_INTERVAL: u64 = 20 * 60 * 1000;

#[derive(Serialize, Deserialize)]
struct KinodeBlogPost {
    slug: String,
    content: String,
    title: String,
    date: u64,
    #[serde(rename = "thumbnailImage")]
    thumbnail_image: String,
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}

call_init!(init);
fn init(_our: Address) {
    // updates: given a static message, the current version of the system,
    // and the kinode.org website, produce a widget for our homepage which
    // presents this information in a visually appealing way.
    loop {
        // add ourselves to the homepage
        Request::to(("our", "homepage", "homepage", "sys"))
            .body(
                serde_json::json!({
                    "Add": {
                        "label": "KinoUpdates",
                        "widget": create_widget(fetch_most_recent_blog_posts(12)),
                    }
                })
                .to_string(),
            )
            .send()
            .unwrap();

        timer::set_and_await_timer(REFRESH_INTERVAL).expect("Failed to produce a timer!");
    }
}

fn create_widget(posts: Vec<KinodeBlogPost>) -> String {
    return format!(
        r#"<html>
<head>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
    * {{
        box-sizing: border-box;
        margin: 0;
        padding: 0;
    }}

    a {{
        text-decoration: none;
        color: inherit;
    }}

    h2 {{
        font-size: medium;
    }}

    body {{
        color: white;
        overflow: hidden;
        height: 100vh;
        width: 100vw;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        font-family: sans-serif;
    }}

    #latest-blog-posts {{
        display: flex;
        flex-direction: column;
        padding: 0.5rem;
        gap: 0.5rem;
        backdrop-filter: brightness(1.25);
        border-radius: 0.75rem;
        box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);
        height: 100vh;
        width: 100vw;
        overflow-y: auto;
        scrollbar-color: transparent transparent;
        scrollbar-width: none;
        align-self: stretch;
    }}

    .post {{
        width: 100%;
        display: flex;
        gap: 8px;
        background-color: rgba(255, 255, 255, 0.1);
        border-radius: 0.5em;
        padding: 0.5em;
    }}

    .post-image {{
        background-size: cover;
        background-repeat: no-repeat;
        background-position: center;
        width: 100px;
        height: 100px;
        border-radius: 4px;
    }}

    .post-info {{
        max-width: 67%;
        overflow: hidden;
    }}

    @media screen and (min-width: 500px) {{
        .post {{
            width: 49%;
        }}
    }}
    </style>
</head>
<body class="text-white overflow-hidden">
    <div
        id="latest-blog-posts"
        style="
            scrollbar-color: transparent transparent;
            scrollbar-width: none;
        ">
        {}
    </div>
</body>
</html>"#,
        posts
            .into_iter()
            .map(post_to_html_string)
            .collect::<String>()
    );
}

fn fetch_most_recent_blog_posts(n: usize) -> Vec<KinodeBlogPost> {
    let blog_posts = match http::send_request_await_response(
        http::Method::GET,
        url::Url::parse("https://kinode.org/api/blog/posts").unwrap(),
        None,
        60,
        vec![],
    ) {
        Ok(response) => serde_json::from_slice::<Vec<KinodeBlogPost>>(response.body())
            .expect("Invalid UTF-8 from kinode.org"),
        Err(e) => panic!("Failed to fetch blog posts: {:?}", e),
    };

    blog_posts.into_iter().rev().take(n as usize).collect()
}

/// Take first 100 chars of a blog post and append "..." to the end
fn trim_content(content: &str) -> String {
    let len = 75;
    if content.len() > len {
        format!("{}...", &content[..len])
    } else {
        content.to_string()
    }
}

fn post_to_html_string(post: KinodeBlogPost) -> String {
    format!(
        r#"<a
            class="post"
            href="https://kinode.org/blog/post/{}"
            target="_blank"
            rel="noopener noreferrer"
        >
        <div
            class="post-image"
            style="background-image: url('https://kinode.org{}-thumbnail');"
        ></div>
        <div class="post-info">
            <h2 class="font-bold">{}</h2>
            <p>{}</p>
        </div>
    </a>"#,
        post.slug,
        post.thumbnail_image,
        post.title,
        trim_content(&post.content),
    )
}
