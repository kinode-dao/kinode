declare module '@ensdomains/eth-ens-namehash' {
    export function hash(name: string): string;
    export function normalize(name: string): string;
}
declare module 'idna-uts46-hx' {
    export function toAscii(domain: string, options?: object): string;
    export function toUnicode(domain: string, options?: object): string;
}