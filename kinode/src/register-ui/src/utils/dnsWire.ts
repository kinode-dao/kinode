export function toDNSWireFormat(domain: string) {
    let parts = domain.split('.');
    let result = new Uint8Array(domain.length + parts.length);
    let idx = 0;
    
    for (let part of parts) {
        let len = part.length;
        result[idx] = len; // write length byte
        idx++;
        for (let j = 0; j < len; j++) {
            result[idx] = part.charCodeAt(j); // write ASCII bytes of the label
            idx++;
        }
    }
    // result[idx] = 0;  // TODO do you need null byte at the end?

    return `0x${Array.from(result).map(byte => byte.toString(16).padStart(2, '0')).join('')}`;
}
