import React, { useState } from 'react';
import { ManifestResponse, PackageManifestEntry } from '../types/Apps';
import { FaChevronDown, FaChevronRight, FaGlobe, FaLock, FaShieldAlt } from 'react-icons/fa';

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


const ProcessManifest: React.FC<{ manifest: PackageManifestEntry }> = ({ manifest }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const hasCapabilities = manifest.request_capabilities.length > 0 || manifest.grant_capabilities.length > 0;

    return (
        <div className="process-manifest">
            <button
                className="process-header"
                onClick={() => setIsExpanded(!isExpanded)}
            >
                {isExpanded ? <FaChevronDown /> : <FaChevronRight />}
                <span className="process-name">{manifest.process_name}</span>
                <div className="process-indicators">
                    {manifest.request_networking && (
                        <FaGlobe title="Requests Network Access" className="network-icon" />
                    )}
                    {hasCapabilities && (
                        <FaShieldAlt title="Has Capability Requirements" className="capability-icon" />
                    )}
                    {!manifest.public && (
                        <FaLock title="Private Process" className="private-icon" />
                    )}
                </div>
            </button>

            {isExpanded && (
                <div className="process-details">
                    {manifest.request_capabilities.length > 0 && (
                        <div className="capability-section">
                            <h4>Requested Capabilities:</h4>
                            <ul>
                                {transformCapabilities(manifest.request_capabilities).map((cap, i) => (
                                    <li key={i}>{cap}</li>
                                ))}
                            </ul>
                        </div>
                    )}

                    {manifest.grant_capabilities.length > 0 && (
                        <div className="capability-section">
                            <h4>Granted Capabilities:</h4>
                            <ul>
                                {transformCapabilities(manifest.grant_capabilities).map((cap, i) => (
                                    <li key={i}>{cap}</li>
                                ))}
                            </ul>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};

const ManifestDisplay: React.FC<ManifestDisplayProps> = ({ manifestResponse }) => {
    if (!manifestResponse) {
        return <p>No manifest data available.</p>;
    }

    const parsedManifests: PackageManifestEntry[] = JSON.parse(manifestResponse.manifest);

    return (
        <div className="manifest-display">
            {parsedManifests.map((manifest, index) => (
                <ProcessManifest key={index} manifest={manifest} />
            ))}
        </div>
    );
};

export default ManifestDisplay;