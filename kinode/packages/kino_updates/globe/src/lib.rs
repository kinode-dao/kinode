use kinode_process_lib::{call_init, http, println, Address, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

call_init!(init);
fn init(_our: Address) {
    // fetch our location with HTTP client
    let location_json = loop {
        match http::send_request_await_response(
            http::Method::GET,
            url::Url::parse("https://ipapi.co/json/").unwrap(),
            Some(std::collections::HashMap::from([(
                "User-Agent".to_string(),
                "ipapi.co/#rust-v1.5".to_string(),
            )])),
            60,
            vec![],
        ) {
            Ok(response) => match serde_json::from_slice::<serde_json::Value>(response.body()) {
                Ok(location) => {
                    if location.get("latitude").is_some() && location.get("longitude").is_some() {
                        break location;
                    } else {
                        println!("Failed to parse location: {location:?}");
                    }
                }
                Err(e) => {
                    println!("Failed to parse location: {e:?}");
                }
            },
            Err(e) => {
                println!("Failed to fetch location: {e:?}");
            }
        };
        std::thread::sleep(std::time::Duration::from_secs(5));
    };

    // add ourselves to the homepage
    Request::to(("our", "homepage", "homepage", "sys"))
        .body(
            serde_json::json!({
                "Add": {
                    "label": "Globe",
                    "widget": create_widget(location_json),
                }
            })
            .to_string(),
        )
        .send()
        .unwrap();
}

fn create_widget(location_json: serde_json::Value) -> String {
    return format!(
        r#"<html>

    <head>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <script src="//unpkg.com/globe.gl"></script>
    </head>

    <body style="margin: 0; width: 100%; height: 100%;">
        <div id="globe" style="margin: 0; width: 100%; height: 100%;"></div>
        <script>
            // Get user's location from IP address and display that point
            const data = {};
            const gData = [{{
                lat: data.latitude,
                lng: data.longitude,
                size: 0.3,
                color: 'red'
            }}];

            Globe()
                .globeImageUrl('//unpkg.com/three-globe/example/img/earth-blue-marble.jpg')
                .pointsData(gData)
                .pointAltitude('size')
                .pointColor('color')
                .pointOfView({{ lat: data.latitude, lng: data.longitude, altitude: 2 }}, 1000)
                (document.getElementById('globe'));
        </script>
    </body>

    </html>"#,
        location_json
    );
}
