import { ReactNode } from "react";
import { IconType } from "react-icons/lib";

export interface PackageId {
    package_name: string;
    publisher_node: string;
}

export interface AppListing {
    package_id: PackageId
    tba: string
    metadata_uri: string
    metadata_hash: string
    metadata?: OnchainPackageMetadata
    auto_update: boolean
}

export type DownloadItem = {
    Dir?: DirItem;
    File?: FileItem;
};

export interface DirItem {
    name: string;
    mirroring: boolean;
}

export interface FileItem {
    name: string;
    size: number;
    manifest: string;
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
    code_hashes: [string, string][];
    license?: string;
    screenshots?: string[];
    wit_version?: number;
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
    package_id: PackageId;
    our_version_hash: string;
    verified: boolean;
    caps_approved: boolean;
    pending_update_hash?: string;
}

export interface PackageManifestEntry {
    process_name: string
    process_wasm_path: string
    on_exit: string
    request_networking: boolean
    request_capabilities: any[]
    grant_capabilities: any[]
    public: boolean
}

export interface ManifestResponse {
    package_id: PackageId;
    version_hash: string;
    manifest: string;
}

export interface HomepageApp {
    id: string;
    process: string;
    package_name: string;
    publisher: string;
    path?: string;
    label: string;
    base64_icon?: string;
    widget?: string;
    order: number;
    favorite: boolean;
}

export interface HashMismatch {
    desired: string;
    actual: string;
}

export type DownloadError =
    | "NoPackage"
    | "NotMirroring"
    | { HashMismatch: HashMismatch }
    | "FileNotFound"
    | "WorkerSpawnFailed"
    | "HttpClientError"
    | "BlobNotFound"
    | "VfsError"
    | "Timeout"
    | "InvalidManifest"
    | "Offline";

export interface UpdateInfo {
    errors: [string, DownloadError][]; // [url/node, error]
    pending_manifest_hash: string | null;
}

export type Updates = {
    [key: string]: { // package_id
        [key: string]: UpdateInfo; // version_hash -> update info
    };
};

export type NotificationActionType = 'click' | 'modal' | 'popup' | 'redirect';

export type NotificationAction = {
    label: string;
    icon?: IconType;
    variant?: 'primary' | 'secondary' | 'danger';
    action: {
        type: NotificationActionType;
        onClick?: () => void;
        modalContent?: ReactNode | (() => ReactNode);
        popupContent?: ReactNode | (() => ReactNode);
        path?: string;
    };
};

export type Notification = {
    id: string;
    type: 'error' | 'success' | 'warning' | 'info' | 'download' | 'install' | 'update';
    message: string;
    timestamp: number;
    metadata?: {
        packageId?: string;
        versionHash?: string;
        progress?: number;
        error?: string;
    };
    actions?: NotificationAction[];
    renderContent?: (notification: Notification) => ReactNode;
    persistent?: boolean;
};