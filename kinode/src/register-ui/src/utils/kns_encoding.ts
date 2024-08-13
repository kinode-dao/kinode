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