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
    // sort contacts alphabetically by node
    Object.entries(contacts).sort((a, b) => a[0].localeCompare(b[0])).forEach(([node, contact]) => {
        const li = document.createElement('li');
        const div = document.createElement('div');
        div.classList.add('contact');
        div.innerHTML = `<h3>${node}</h3>
        <ul>
        ${Object.entries(contact).sort((a, b) => a[0].localeCompare(b[0])).map(([field, value]) => `
            <li>
                ${field}: ${JSON.stringify(value)}
                <button class="remove-field" onclick="removeField('${node}', '${field}')">X</button>
            </li>
        `).join('')}
        </ul>
        <form class="delete-contact" id="${node}">
            <button type="submit">delete</button>
        </form>
        <form class="add-field" id="${node}">
            <input type="text" name="field" placeholder="field (e.g. name)">
            <input type="text" name="value" placeholder="value (e.g. John Doe)" title="Enter any valid JSON value (e.g. &quot;John Doe&quot;, 42, true, [1,2,3], {&quot;key&quot;:&quot;value&quot;})">
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
            let value = data.get('value');
            // if value is not valid JSON, wrap it in quotes
            try {
                JSON.parse(value);
            } catch (e) {
                // If parsing fails, assume it's a string and wrap it in quotes
                value = `"${value}"`;
            }
            api_call({
                "AddField": [node, data.get('field'), value]
            });
        });
    });
}

document.getElementById('back-button').addEventListener('click', () => {
    // set page to `/` while also removing the subdomain
    window.location.href = window.location.origin.replace('//contacts-sys.', '//') + '/'
});

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

function removeField(node, field) {
    api_call({
        "RemoveField": [node, field]
    });
}

// Setup WebSocket connection
const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
const ws = new WebSocket(wsProtocol + location.host + "/contacts:contacts:sys/");
ws.onmessage = event => {
    const data = JSON.parse(event.data);
    populate(data);
};

