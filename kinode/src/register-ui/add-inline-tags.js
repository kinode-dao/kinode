const fs = require('fs');
const path = require('path');

const indexPath = path.join(__dirname, 'build', 'index.html');

fs.readFile(indexPath, 'utf8', (err, data) => {
    if (err) {
        console.error(err);
        return;
    }

    let modifiedHtml = data
        .replace(/<script src="(.*?)"><\/script>/g, '<script src="$1" inline></script>')
        .replace(/<link href="(.*?)" rel="stylesheet">/g, '<link href="$1" rel="stylesheet" inline>');

    fs.writeFile(indexPath, modifiedHtml, 'utf8', (err) => {
        if (err) return console.log(err);
    });
});
