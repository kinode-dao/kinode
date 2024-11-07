import React from 'react';
import { ManifestResponse, PackageManifestEntry } from '../types/Apps';

interface ManifestDisplayProps {
    manifestResponse: ManifestResponse;
}

const capabilityMap: Record<string, string> = {
    'vfs:distro:sys': 'Virtual Filesystem',
    'http_client:distro:sys': 'HTTP Client',
    'http_server:distro:sys': 'HTTP Server',
    'eth:distro:sys': 'Ethereum RPC access',
    'homepage:homepage:sys': 'Ability to add itself to homepage',
    'main:app_store:sys': 'App Store',
    'chain:app_store:sys': 'Chain',
    'terminal:terminal:sys': 'Terminal',
};

// note: we can do some future regex magic mapping here too!
// if includes("root") return WARNING
const transformCapabilities = (capabilities: any[]) => {
    return capabilities.map(cap => capabilityMap[cap] || cap);
};


const renderManifest = (manifest: PackageManifestEntry) => (
    <div className="manifest-item">
        <p><strong>Process Name:</strong> {manifest.process_name}</p>
        <p><strong>Requests Network Access:</strong> {manifest.request_networking ? 'Yes' : 'No'}</p>
        <p><strong>Wants to Grant Capabilities:</strong> {transformCapabilities(manifest.grant_capabilities).join(', ') || 'None'}</p>
        <p><strong>Is Public:</strong> {manifest.public ? 'Yes' : 'No'}</p>
        <p><strong>Requests the Capabilities:</strong> {transformCapabilities(manifest.request_capabilities).join(', ') || 'None'}</p>
        {/* <p><strong>Process Wasm Path:</strong> {manifest.process_wasm_path}</p> */}
        {/* <p><strong>On Exit:</strong> {manifest.on_exit}</p> */}
    </div>
);

const ManifestDisplay: React.FC<ManifestDisplayProps> = ({ manifestResponse }) => {
    if (!manifestResponse) {
        return <p>No manifest data available.</p>;
    }

    const parsedManifests: PackageManifestEntry[] = JSON.parse(manifestResponse.manifest);

    return (
        <div className="manifest-display">
            {parsedManifests.map((manifest, index) => (
                <div key={index}>
                    {renderManifest(manifest)}
                </div>
            ))}
        </div>
    );
};

export default ManifestDisplay;