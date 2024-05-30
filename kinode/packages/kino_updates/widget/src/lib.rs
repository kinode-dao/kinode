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
                        "widget": create_widget(fetch_most_recent_blog_posts(6)),
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
    <script src="https://cdn.tailwindcss.com"></script>
    <style>
        .post {{
            width: 100%;
        }}

        .post-image {{
            background-size: cover;
            background-repeat: no-repeat;
            background-position: center;
            width: 100px;
            height: 100px;
            border-radius: 16px;
        }}

        .post-info {{
            max-width: 67%
        }}

        @media screen and (min-width: 500px) {{
            .post {{
                width: 49%;
            }}
        }}
    </style>
</head>
<body class="text-white overflow-hidden h-screen w-screen flex flex-col gap-2">
    <div
        id="latest-blog-posts"
        class="flex flex-col p-2 gap-2 backdrop-brightness-125 rounded-xl shadow-lg h-screen w-screen overflow-y-auto self-stretch"
        style="
            scrollbar-color: transparent transparent;
            scrollbar-width: none;
        "
    >
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
            class="post p-2 grow self-stretch flex items-stretch rounded-lg shadow bg-white/10 font-sans w-full"
            href="https://kinode.org/blog/post/{}"
            target="_blank" 
            rel="noopener noreferrer"
        >
        <div
            class="post-image rounded mr-2 grow self-stretch h-full"
            style="background-image: url('https://kinode.org{}');"
        ></div>
        <div class="post-info flex flex-col grow">
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
