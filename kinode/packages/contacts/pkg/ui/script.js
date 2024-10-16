const APP_PATH = '/contacts:contacts:sys/ask';

// Fetch initial data and populate the UI
function init() {
    fetch(APP_PATH)
        .then(response => response.json())
        .then(data => {
            populate(data);
        });
}

function api_call(body) {
    fetch(APP_PATH, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
    });
}

function populate(data) {
    console.log(data);
    populate_contacts(data);
}

function populate_contacts(contacts) {
    const ul = document.getElementById('contacts');
    ul.innerHTML = '';
    Object.entries(contacts).forEach(([node, contact]) => {
        const li = document.createElement('li');
        li.innerHTML = `${JSON.stringify(node, undefined, 2)}`;
        ul.appendChild(li);
    });
}

document.getElementById('add-contact').addEventListener('submit', (e) => {
    e.preventDefault();
    const data = new FormData(e.target);
    const node = data.get('node');
    const body = {
        "AddContact": node
    };
    fetch(APP_PATH, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
    }).then(response => {
        if (response.status === 200) {
            return null;
        } else {
            return response.json();
        }
    }).then(data => {
        if (data === null) {
            e.target.reset();
            return;
        } else {
            alert(JSON.stringify(data));
        }
    }).catch(error => {
        console.error('Error:', error);
    });
})

// Call init to start the application
init();

// Setup WebSocket connection
const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
const ws = new WebSocket(wsProtocol + location.host + "/contacts:contacts:sys/");
ws.onmessage = event => {
    const data = JSON.parse(event.data);
    populate(data);
};

