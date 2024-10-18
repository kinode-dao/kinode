const APP_PATH = '/contacts:contacts:sys/ask';

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
        const div = document.createElement('div');
        div.classList.add('contact');
        div.innerHTML = `<h3>${node}</h3>
        <ul>
        ${Object.entries(contact).map(([field, value]) => `<li>${field}: ${value}</li>`).join('')}
        </ul>
        <form class="delete-contact" id="${node}">
            <button type="submit">delete</button>
        </form>
        <form class="add-field" id="${node}">
            <input type="text" name="field" placeholder="Field">
            <input type="text" name="value" placeholder="Value">
            <button type="submit">add</button>
        </form>
        `;
        li.appendChild(div);
        ul.appendChild(li);
    });

    ul.querySelectorAll('.delete-contact').forEach(form => {
        form.addEventListener('submit', function (e) {
            e.preventDefault();
            const node = this.getAttribute('id');
            api_call({
                "RemoveContact": node
            });
        });
    });

    ul.querySelectorAll('.add-field').forEach(form => {
        form.addEventListener('submit', function (e) {
            e.preventDefault();
            const node = this.getAttribute('id');
            const data = new FormData(e.target);
            api_call({
                "AddField": [node, data.get('field'), data.get('value')]
            });
        });
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
        e.target.reset();
        if (response.status === 200) {
            return null;
        } else {
            return response.json();
        }
    }).then(data => {
        if (data === null) {
            return;
        } else {
            alert(JSON.stringify(data));
        }
    }).catch(error => {
        console.error('Error:', error);
    });
})

// Setup WebSocket connection
const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
const ws = new WebSocket(wsProtocol + location.host + "/contacts:contacts:sys/");
ws.onmessage = event => {
    const data = JSON.parse(event.data);
    populate(data);
};

