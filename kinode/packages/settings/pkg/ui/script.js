const APP_PATH = '/settings:settings:sys/ask';

// Fetch initial data and populate the UI
function init() {
    fetch('/our')
        .then(response => response.text())
        .then(data => {
            const our = data + '@settings:settings:sys';
            fetch(APP_PATH)
                .then(response => response.json())
                .then(data => {
                    console.log(data);
                });
        });
}

// Call init to start the application
init();

// Setup WebSocket connection
const ws = new WebSocket("ws://" + location.host + "/settings:settings:sys/");
ws.onmessage = event => {
    const data = JSON.parse(event.data);
    console.log(data);
};

