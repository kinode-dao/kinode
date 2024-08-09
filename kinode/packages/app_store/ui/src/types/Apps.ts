export interface MyApps {
    downloaded: AppInfo[]
    installed: AppInfo[]
    local: AppInfo[]
    system: AppInfo[]
}

export interface AppListing {
    tba: string
    package: string
    publisher: string
    metadata_hash: string
    metadata_uri: string
    metadata?: OnchainPackageMetadata
    installed: boolean
    state?: PackageState
}

export interface MirrorCheckFile {
    node: string;
    is_online: boolean;
    error: string | null;
}

export interface Erc721Properties {
    package_name: string;
    publisher: string;
    current_version: string;
    mirrors: string[];
    code_hashes: Record<string, string>;
    license?: string;
    screenshots?: string[];
    wit_version?: [number, number, number];
}

export interface OnchainPackageMetadata {
    name?: string;
    description?: string;
    image?: string;
    external_url?: string;
    animation_url?: string;
    properties: Erc721Properties;
}

export interface PackageState {
    mirrored_from: string;
    our_version: string;
    installed: boolean;
    verified: boolean;
    caps_approved: boolean;
    manifest_hash?: string;
    mirroring: boolean;
    auto_update: boolean;
    // source_zip?: Uint8Array, // bytes
}

export interface AppInfo extends AppListing {
    permissions?: string[]
}

export interface PackageManifest {
    process_name: string
    process_wasm_path: string
    on_exit: string
    request_networking: boolean
    request_capabilities: any[]
    grant_capabilities: any[]
    public: boolean
}

[
    {
        "installed": false,
        "metadata": null,
        "metadata_hash": "0xf244e4e227494c6a0716597f0c405284eb53f7916427d48ceb03a24ed5b52b5d",
        "owner": "0x7Bf904E36715B650Fb1F99113cb4A2B2FfE22392",
        "package": "sdapi",
        "publisher": "mothu-et-doria.os",
        "state": null
    },
    {
        "installed": false,

        "metadata_hash": "0xe43f616b39f2511f2c3c29c801a0993de5a74ab1fc4382ff7c68aad50f0242f3",
        "owner": "0xDe12193c037F768fDC0Db0B77B7E70de723b95E7",
        "package": "chat",
        "publisher": "mythicengineer.os",
        "state": null
    },
    {
        "installed": false,
        "metadata": null,
        "metadata_hash": "0x4385b4b9ddddcc25ce99d6ae1542b1362c0e7f41abf1385cd9eda4d39ced6e39",
        "owner": "0x7213aa2A6581b37506C035b387b4Bf2Fb93E2f88",
        "package": "chat_template",
        "publisher": "odinsbadeye.os",
        "state": null
    },
    {
        "installed": false,
        "metadata": null,
        "metadata_hash": "0x0f4c02462407d88fb43a0e24df7e36b7be4a09f2fc27bb690e5b76c8d21088ef",
        "owner": "0x958946dEcCfe3546fE7F3f98eb07c100E472F09D",
        "package": "kino_files",
        "publisher": "gloriainexcelsisdeo.os",
        "state": null
    }
]
