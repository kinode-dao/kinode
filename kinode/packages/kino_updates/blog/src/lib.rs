use kinode_process_lib::{call_init, http, timer, Address, Request};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

/// 2 hours
const REFRESH_INTERVAL: u64 = 120 * 60 * 1000;

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
                        "label": "Updates from kinode.org",
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
    <link rel="stylesheet" href="/kinode.css">
    <style>
    * {{
        box-sizing: border-box;
        margin: 0;
        padding: 0;
        font-family: 'Kode Mono', monospace;
    }}

    h2 {{
        font-size: small;
    }}

    p {{
        font-size: small;
    }}

    body {{
        overflow: hidden;
        height: 100vh;
        width: 100vw;
        display: flex;
        flex-direction: column;
        gap: 5rem;
        background: transparent;
    }}

    #latest-blog-posts {{
        display: flex;
        flex-direction: column;
        padding-left: 1em;
        height: 100vh;
        width: 100vw;
        overflow-y: auto;
        scrollbar-color: transparent transparent;
        scrollbar-width: none;
        align-self: stretch;
        padding-bottom: 30px;
    }}

    .post {{
        width: 100%;
        display: flex;
        gap: 8px;
        padding: 1em 1em 1em 0em;
        border-bottom: 1px solid rgba(0,0,0,0.1)
    }}

    .post-image {{
        background-size: cover;
        background-repeat: no-repeat;
        background-position: center;
        width: 100px;
        height: 100px;
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
<body>
    <div id="latest-blog-posts">
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
    let blog_posts = match http::client::send_request_await_response(
        http::Method::GET,
        url::Url::parse("https://kinode.org/api/blog/posts").unwrap(),
        None,
        60,
        vec![],
    ) {
        Ok(response) => match serde_json::from_slice::<Vec<KinodeBlogPost>>(response.body()) {
            Ok(posts) => posts,
            Err(e) => {
                println!("Failed to parse blog posts: {e:?}");
                vec![]
            }
        },
        Err(e) => {
            println!("Failed to fetch blog posts: {e:?}");
            vec![]
        }
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
