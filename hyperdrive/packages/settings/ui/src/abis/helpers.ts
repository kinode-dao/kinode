import { encodeFunctionData, encodePacked, stringToHex, bytesToHex } from "viem";
import { hypermapAbi } from "./";
import sha3 from 'js-sha3';
import { toUnicode } from 'idna-uts46-hx';

export function noteFunction(label: string, value: string) {
    console.log(label, value);

    if (label === "~ip") {
        value = bytesToHex(ipToBytes(value));
    } else if (label === "~ws-port" || label === "~tcp-port") {
        value = bytesToHex(portToBytes(parseInt(value)));
    } else if (label === "~routers") {
        value = encodeRouters(value.split(','));
    }

    return encodeFunctionData({
        abi: hypermapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex(label)]),
            encodePacked(["bytes"], [value.startsWith('0x') ? value as `0x${string}` : stringToHex(value)]),
        ]
    });
}

const encodeRouters = (routers: string[]): `0x${string}` => {
    const hashedRouters = routers.map(router => kinohash(router).slice(2)); // Remove '0x' prefix
    return `0x${hashedRouters.join('')}`;
};

export const kinohash = (inputName: string): `0x${string}` =>
    ('0x' + normalize(inputName)
        .split('.')
        .reverse()
        .reduce(reducer, '00'.repeat(32))) as `0x${string}`;

const reducer = (node: string, label: string): string =>
    sha3.keccak_256(Buffer.from(node + sha3.keccak_256(label), 'hex'));

export const normalize = (name: string): string => {
    const tilde = name.startsWith('~');
    const clean = tilde ? name.slice(1) : name;
    const normalized = clean ? unicode(clean) : clean;
    return tilde ? '~' + normalized : normalized;
};

const unicode = (name: string): string =>
    toUnicode(name, { useSTD3ASCIIRules: true })

export function bytesToIp(bytes: Uint8Array): string {
    if (bytes.length !== 16) {
        throw new Error('Invalid byte length for IP address');
    }

    const view = new DataView(bytes.buffer);
    const ipNum = view.getBigUint64(0) * BigInt(2 ** 64) + view.getBigUint64(8);

    if (ipNum < BigInt(2 ** 32)) {
        // IPv4
        return new Uint8Array(bytes.slice(12)).join('.');
    } else {
        // IPv6
        const parts: string[] = [];
        for (let i = 0; i < 8; i++) {
            parts.push(view.getUint16(i * 2).toString(16));
        }
        return parts.join(':');
    }
}

export function ipToBytes(ip: string): Uint8Array {
    if (ip.includes(':')) {
        // IPv6: Create a 16-byte array
        const bytes = new Uint8Array(16);
        const view = new DataView(bytes.buffer);
        const parts = ip.split(':');
        for (let i = 0; i < 8; i++) {
            view.setUint16(i * 2, parseInt(parts[i] || '0', 16));
        }
        return bytes;
    } else {
        // IPv4: Create a 4-byte array
        const bytes = new Uint8Array(4);
        const parts = ip.split('.');
        for (let i = 0; i < 4; i++) {
            bytes[i] = parseInt(parts[i], 10);
        }
        return bytes;
    }
}

export function bytesToPort(bytes: Uint8Array): number {
    if (bytes.length !== 2) {
        throw new Error('Invalid byte length for port');
    }
    return new DataView(bytes.buffer).getUint16(0);
}

export function portToBytes(port: number): Uint8Array {
    if (port < 0 || port > 65535) {
        throw new Error('Invalid port number');
    }
    const bytes = new Uint8Array(2);
    new DataView(bytes.buffer).setUint16(0, port);
    return bytes;
}