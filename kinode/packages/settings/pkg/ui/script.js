const APP_PATH = '/settings:settings:sys/ask';

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

function shutdown() {
    api_call("Shutdown");
    setTimeout(() => {
        window.location.reload();
    }, 1000);
}

function populate(data) {
    populate_node_info(data.identity);
    populate_net_diagnostics(data.diagnostics);
    populate_eth_rpc_providers(data.eth_rpc_providers);
    populate_eth_rpc_settings(data.eth_rpc_access_settings);
    populate_process_map(data.process_map);
    populate_stylesheet_editor(data.stylesheet);
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

function populate_process_map(process_map) {
    // apps we don't want user to kill, also runtime modules that cannot be killed
    const do_not_kill = [
        'settings:setting:sys',
        'main:app_store:sys',
        'net:distro:sys',
        'kernel:distro:sys',
        'kv:distro:sys',
        'sqlite:distro:sys',
        'eth:distro:sys',
        'vfs:distro:sys',
        'state:distro:sys',
        'kns_indexer:kns_indexer:sys',
        'http_client:distro:sys',
        'http_server:distro:sys',
        'terminal:terminal:sys',
        'timer:distro:sys',
    ];

    const ul = document.getElementById('process-map');
    ul.innerHTML = '';
    Object.entries(process_map).forEach(([id, process]) => {
        const li = document.createElement('li');

        const toggleButton = document.createElement('button');
        toggleButton.textContent = `${id}`;
        toggleButton.onclick = function () {
            const details = this.nextElementSibling;
            details.style.display = details.style.display === 'none' ? 'block' : 'none';
        };
        li.appendChild(toggleButton);

        const detailsDiv = document.createElement('div');
        detailsDiv.style.display = 'none';

        if (!do_not_kill.includes(id)) {
            const killButton = document.createElement('button');
            killButton.className = 'kill-process';
            killButton.setAttribute('data-id', id);
            killButton.textContent = 'kill';
            detailsDiv.appendChild(killButton);
        }

        const publicInfo = document.createElement('p');
        publicInfo.textContent = `public: ${process.public}`;
        detailsDiv.appendChild(publicInfo);

        const onExit = document.createElement('p');
        onExit.textContent = `on_exit: ${process.on_exit}`;
        detailsDiv.appendChild(onExit);

        if (process.wit_version) {
            const witVersion = document.createElement('p');
            witVersion.textContent = `wit_version: ${process.wit_version}`;
            detailsDiv.appendChild(witVersion);
        }

        if (process.wasm_bytes_handle) {
            const wasmBytesHandle = document.createElement('p');
            wasmBytesHandle.textContent = `wasm_bytes_handle: ${process.wasm_bytes_handle}`;
            detailsDiv.appendChild(wasmBytesHandle);
        }

        const capsList = document.createElement('ul');
        process.capabilities.forEach(cap => {
            const capLi = document.createElement('li');
            capLi.textContent = `${cap.issuer}(${JSON.stringify(JSON.parse(cap.params), null, 2)})`;
            capsList.appendChild(capLi);
        });
        detailsDiv.appendChild(capsList);

        li.appendChild(detailsDiv);
        ul.appendChild(li);
    });
    document.querySelectorAll('.kill-process').forEach(button => {
        button.addEventListener('click', () => {
            api_call({ "KillProcess": button.getAttribute('data-id') });
        });
    });
}

function populate_stylesheet_editor(stylesheet) {
    document.getElementById('stylesheet-editor').value = stylesheet;
}

function save_stylesheet() {
    const stylesheet = document.getElementById('stylesheet-editor').value;
    api_call({ "SetStylesheet": stylesheet });
}

// Call init to start the application
init();

// Setup event listeners
document.getElementById('shutdown').addEventListener('click', shutdown);

document.getElementById('save-stylesheet').addEventListener('click', save_stylesheet);

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
                e.target.reset();
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
                e.target.reset();
                document.getElementById('peer-ping-response').innerText = "ping successful!";
            } else if (data === "HiTimeout") {
                document.getElementById('peer-ping-response').innerText = "node timed out";
            } else if (data === "HiOffline") {
                document.getElementById('peer-ping-response').innerText = "node is offline";
            }
        });
})

document.getElementById('add-eth-provider').addEventListener('submit', (e) => {
    e.preventDefault();
    const data = new FormData(e.target);
    const rpc_url = data.get('rpc-url');
    // validate rpc url
    if (!rpc_url.startsWith('wss://') && !rpc_url.startsWith('ws://')) {
        alert('Invalid RPC URL');
        return;
    }
    const body = {
        "EthConfig": {
            "AddProvider": {
                chain_id: Number(data.get('chain-id')),
                trusted: false,
                provider: { "RpcUrl": rpc_url },
            }
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
                e.target.reset();
                return;
            } else {
                alert(data);
            }
        });
})

document.getElementById('remove-eth-provider').addEventListener('submit', (e) => {
    e.preventDefault();
    const data = new FormData(e.target);
    const body = {
        "EthConfig": {
            "RemoveProvider": [Number(data.get('chain-id')), data.get('rpc-url')]
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
                e.target.reset();
                return;
            } else {
                alert(data);
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

