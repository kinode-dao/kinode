const APP_PATH = '/contacts:contacts:sys/';

// Fetch initial data and populate the UI
function init() {
    fetch(APP_PATH + 'get')
        .then(response => response.json())
        .then(data => {
            populate(data);
        });
}

function api_call(path, body) {
    fetch(APP_PATH + path, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
    });
}

function populate(data) {
    console.log(data);
}

// Call init to start the application
init();

// Setup WebSocket connection
// const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
// const ws = new WebSocket(wsProtocol + location.host + "/settings:settings:sys/");
// ws.onmessage = event => {
//     const data = JSON.parse(event.data);
//     console.log(data);
//     populate(data);
// };

