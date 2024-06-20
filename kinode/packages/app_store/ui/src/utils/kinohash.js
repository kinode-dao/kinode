import sha3 from 'js-sha3'
import { toUnicode } from 'idna-uts46-hx'

export const kinohash = (inputName) =>
    '0x' + normalize(inputName)
        .split('.')
        .reverse()
        .reduce(reducer, '00'.repeat(32));

const reducer = (node, label) =>
    sha3.keccak_256(new Buffer.from(node + sha3.keccak_256(label), 'hex'));

export const normalize = (name) => {
    const tilde = name.startsWith('~');
    const clean = tilde ? name.slice(1) : name;
    const normalized = clean ? unicode(clean) : clean;
    return tilde ? '~' + normalized : normalized;
};

const unicode = (name) =>
    toUnicode(name, { useStd3ASCII: true, transitional: false });
