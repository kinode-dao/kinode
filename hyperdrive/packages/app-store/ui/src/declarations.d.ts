declare module 'idna-uts46-hx' {
    export function toAscii(domain: string, options?: object): string;
    export function toUnicode(domain: string, options?: object): string;
}

declare module '@ensdomains/eth-ens-namehash' {
    export function hash(name: string): string;
    export function normalize(name: string): string;
}

declare interface ImportMeta {
    env: {
        VITE_OPTIMISM_RPC_URL: string;
        VITE_SEPOLIA_RPC_URL: string;
        BASE_URL: string;
        VITE_NODE_URL?: string;
        DEV: boolean;
    };
}
declare interface Window {
    our: {
        node: string;
        process: string;
    };
}