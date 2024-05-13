const APP_PATH = '/settings:settings:sys/ask';

// Fetch initial data and populate the UI
function init() {
    fetch(APP_PATH)
        .then(response => response.json())
        .then(data => {
            console.log(data);
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

function shutdown() {
    api_call("Shutdown");
}

function populate(data) {
    populate_node_info(data.identity);
    populate_net_diagnostics(data.diagnostics);
    populate_eth_rpc_providers(data.eth_rpc_providers);
    populate_eth_rpc_settings(data.eth_rpc_access_settings);
    // populate_kernel()
}

function populate_node_info(identity) {
    document.getElementById('node-name').innerText = identity.name;
    document.getElementById('net-key').innerText = identity.networking_key;
    if (identity.ws_routing) {
        document.getElementById('ip-ports').innerText = identity.ws_routing;
    } else {
        document.getElementById('ip-ports').style.display = 'none';
    }
    if (identity.routers) {
        document.getElementById('routers').innerText = identity.routers;
    } else {
        document.getElementById('routers').style.display = 'none';
    }
}

function populate_net_diagnostics(diagnostics) {
    document.getElementById('diagnostics').innerText = diagnostics;
}

function populate_eth_rpc_providers(providers) {
    const ul = document.getElementById('providers');
    ul.innerHTML = '';
    providers.forEach(provider => {
        const li = document.createElement('li');
        li.innerHTML = `${JSON.stringify(provider, undefined, 2)}`;
        ul.appendChild(li);
    });
}

function populate_eth_rpc_settings(settings) {
    if (settings.public) {
        document.getElementById('public').innerText = 'status: public';
        document.getElementById('allowed-nodes').style.display = 'none';
    } else {
        document.getElementById('public').innerText = 'status: private';
        const ul = document.getElementById('allowed-nodes');
        ul.innerHTML = '';
        if (settings.allow.length === 0) {
            const li = document.createElement('li');
            li.innerHTML = `<li>(none)</li>`;
            ul.appendChild(li);
        } else {
            settings.allow.forEach(allowed_node => {
                const li = document.createElement('li');
                li.innerHTML = `<li>${allowed_node}</li>`;
                ul.appendChild(li);
            });
        }
    }
    const ul = document.getElementById('denied-nodes');
    ul.innerHTML = '';
    if (settings.deny.length === 0) {
        const li = document.createElement('li');
        li.innerHTML = `<li>(none)</li>`;
        ul.appendChild(li);
    } else {
        settings.deny.forEach(denied_node => {
            const li = document.createElement('li');
            li.innerHTML = `<li>${denied_node}</li>`;
            ul.appendChild(li);
        });
    }
}

// Call init to start the application
init();

// Setup event listeners
document.getElementById('shutdown').addEventListener('click', shutdown);

document.getElementById('get-peer-pki').addEventListener('submit', (e) => {
    e.preventDefault();
    const data = new FormData(e.target);
    const body = {
        "PeerId": data.get('peer'),
    };
    fetch(APP_PATH, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
    }).then(response => response.json())
        .then(data => {
            if (data === null) {
                document.getElementById('peer-pki-response').innerText = "no pki data for peer";
            } else {
                document.getElementById('peer-pki-response').innerText = JSON.stringify(data, undefined, 2);
            }
        });
})

document.getElementById('ping-peer').addEventListener('submit', (e) => {
    e.preventDefault();
    const data = new FormData(e.target);
    const body = {
        "Hi": {
            node: data.get('peer'),
            content: data.get('content'),
            timeout: Number(data.get('timeout')),
        }
    };
    fetch(APP_PATH, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
    }).then(response => response.json())
        .then(data => {
            if (data === null) {
                document.getElementById('peer-ping-response').innerText = "ping successful!";
            } else if (data === "HiTimeout") {
                document.getElementById('peer-ping-response').innerText = "node timed out";
            } else if (data === "HiOffline") {
                document.getElementById('peer-ping-response').innerText = "node is offline";
            }
        });
})

// Setup WebSocket connection
const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
const ws = new WebSocket(wsProtocol + location.host + "/settings:settings:sys/");
ws.onmessage = event => {
    const data = JSON.parse(event.data);
    console.log(data);
    populate(data);
};

