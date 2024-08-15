import React, { useState, useEffect } from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaSpinner, FaRocket, FaChevronDown, FaChevronUp, FaExclamationTriangle } from "react-icons/fa";
import useAppsStore from "../store";
import { DownloadItem, PackageManifest, AppListing } from "../types/Apps";

export default function DownloadPage() {
    const { id } = useParams();
    const { listings, fetchListings, fetchDownloadsForApp, downloadApp, installApp, checkMirror, fetchInstalled, installed, activeDownloads } = useAppsStore();
    const [downloads, setDownloads] = useState<DownloadItem[]>([]);
    const [selectedMirror, setSelectedMirror] = useState<string | null>(null);
    const [mirrorStatuses, setMirrorStatuses] = useState<{ [mirror: string]: { status: 'unchecked' | 'checking' | 'online' | 'offline' } }>({});
    const [isDownloading, setIsDownloading] = useState(false);
    const [isInstalling, setIsInstalling] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [showMetadata, setShowMetadata] = useState(false);
    const [showManifest, setShowManifest] = useState(false);
    const [showCaps, setShowCaps] = useState(false);
    const [manifest, setManifest] = useState<PackageManifest | null>(null);
    const [showCapApproval, setShowCapApproval] = useState(false);
    const [selectedVersion, setSelectedVersion] = useState<string | null>(null);
    const [showManualMirror, setShowManualMirror] = useState(false);
    const [manualMirror, setManualMirror] = useState("");

    const app = listings[id as string];

    useEffect(() => {
        fetchListings();
        fetchInstalled();
        if (id) {
            fetchDownloadsForApp(id).then(setDownloads);
        }
    }, [id, fetchListings, fetchDownloadsForApp, fetchInstalled]);

    useEffect(() => {
        if (app) {
            initializeMirrors();
        }
    }, [app]);

    const initializeMirrors = () => {
        if (!app) return;
        const mirrors = [app.package_id.publisher_node, ...(app.metadata?.properties?.mirrors || [])];
        const initialStatuses: { [mirror: string]: { status: 'unchecked' | 'checking' | 'online' | 'offline' } } = {};
        mirrors.forEach(mirror => {
            initialStatuses[mirror] = { status: 'unchecked' };
        });
        setMirrorStatuses(initialStatuses);
        setSelectedMirror(app.package_id.publisher_node);
        mirrors.forEach(checkMirrorStatus);
    };

    const checkMirrorStatus = async (mirror: string) => {
        setMirrorStatuses(prev => ({ ...prev, [mirror]: { status: 'checking' } }));
        try {
            const status = await checkMirror(mirror);
            setMirrorStatuses(prev => ({ ...prev, [mirror]: { status: status.is_online ? 'online' : 'offline' } }));
        } catch (error) {
            setMirrorStatuses(prev => ({ ...prev, [mirror]: { status: 'offline' } }));
        }
    };

    const handleDownload = async (version: string, hash: string) => {
        if (!app || !selectedMirror) return;
        setIsDownloading(true);
        setError(null);
        try {
            await downloadApp(`${app.package_id.package_name}:${app.package_id.publisher_node}`, hash, selectedMirror);
            const updatedDownloads = await fetchDownloadsForApp(id!);
            setDownloads(updatedDownloads);
        } catch (error) {
            console.error('Download failed:', error);
            setError(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsDownloading(false);
        }
    };

    const handleInstall = async (version: string, hash: string) => {
        if (!app) return;
        setSelectedVersion(hash);  // Store the hash instead of the version
        const download = downloads.find(d => d.File && d.File.name === `${hash}.zip`);
        if (download && download.File) {
            try {
                const manifestData = JSON.parse(download.File.manifest);
                setManifest(manifestData);
                setShowCapApproval(true);
            } catch (error) {
                console.error('Failed to parse manifest:', error);
                setError(`Failed to parse manifest: ${error instanceof Error ? error.message : String(error)}`);
            }
        } else {
            setError('Download not found or manifest missing');
        }
    };

    const confirmInstall = async () => {
        if (!app || !selectedVersion) return;
        setIsInstalling(true);
        setError(null);
        try {
            await installApp(`${app.package_id.package_name}:${app.package_id.publisher_node}`, selectedVersion);
            fetchInstalled();
            setShowCapApproval(false);
        } catch (error) {
            console.error('Installation failed:', error);
            setError(`Installation failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsInstalling(false);
        }
    };

    const renderDownloadProgress = (version: string, hash: string) => {
        const downloadKey = `${app.package_id.package_name}:${app.package_id.publisher_node}:${hash}`;
        const progress = activeDownloads[downloadKey];

        if (progress) {
            const percentage = Math.round((progress.downloaded / progress.total) * 100);
            return (
                <div className="download-progress">
                    <div className="progress-bar" style={{ width: `${percentage}%` }}></div>
                    <span>{percentage}%</span>
                </div>
            );
        }
        return null;
    };

    if (!app) {
        return <div className="downloads-page"><h4>App details not found for {id}</h4></div>;
    }

    return (
        <div className="downloads-page">
            <div className="app-header">
                {app.metadata?.image && (
                    <img src={app.metadata.image} alt={app.metadata?.name || app.package_id.package_name} className="app-icon" />
                )}
                <div className="app-title">
                    <h2>{app.metadata?.name || app.package_id.package_name}</h2>
                    <p className="app-id">{`${app.package_id.package_name}.${app.package_id.publisher_node}`}</p>
                </div>
            </div>

            <div className="app-description">{app.metadata?.description || "No description available"}</div>

            <div className="mirror-selection">
                <h3>Select Mirror</h3>
                <select
                    value={selectedMirror === manualMirror ? 'manual' : selectedMirror || ''}
                    onChange={(e) => {
                        if (e.target.value === 'manual') {
                            setShowManualMirror(true);
                        } else {
                            setSelectedMirror(e.target.value);
                            setShowManualMirror(false);
                            setManualMirror('');
                        }
                    }}
                    disabled={isDownloading}
                >
                    <option value="" disabled>Select Mirror</option>
                    {Object.entries(mirrorStatuses).map(([mirror, { status }]) => (
                        <option key={mirror} value={mirror} disabled={status === 'offline'}>
                            {mirror} {status === 'checking' ? '(Checking...)' : status === 'online' ? '(Online)' : status === 'offline' ? '(Offline)' : ''}
                        </option>
                    ))}
                    <option value="manual">Manual Mirror (Advanced)</option>
                </select>
                {(showManualMirror || selectedMirror === manualMirror) && (
                    <div className="manual-mirror-input">
                        <input
                            type="text"
                            value={manualMirror}
                            onChange={(e) => setManualMirror(e.target.value)}
                            placeholder="Enter mirror node"
                        />
                        <button
                            onClick={() => {
                                if (manualMirror) {
                                    setSelectedMirror(manualMirror);
                                    setShowManualMirror(true);
                                }
                            }}
                            disabled={!manualMirror}
                        >
                            Set Manual Mirror
                        </button>
                        <button
                            onClick={() => {
                                setSelectedMirror(app.package_id.publisher_node);
                                setShowManualMirror(false);
                                setManualMirror('');
                            }}
                        >
                            Reset to Default Mirror
                        </button>
                    </div>
                )}
            </div>


            <h3>Available Versions</h3>
            <table className="downloads-table">
                <thead>
                    <tr>
                        <th>Version</th>
                        <th>Status</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {app.metadata?.properties?.code_hashes.map(([version, hash]) => {
                        const download = downloads.find(d => {
                            if (d.File) {
                                return d.File.name === `${hash}.zip`;
                            }
                            return false;
                        });
                        const isDownloaded = !!download;
                        const isInstalled = installed[id as string]?.our_version_hash === hash;
                        const isDownloading = !!activeDownloads[`${app.package_id.package_name}:${app.package_id.publisher_node}:${hash}`];

                        return (
                            <tr key={version}>
                                <td>{version}</td>
                                <td>
                                    {isInstalled ? 'Installed' : isDownloaded ? 'Downloaded' : isDownloading ? 'Downloading' : 'Not downloaded'}
                                </td>
                                <td>
                                    {!isDownloaded && !isDownloading && (
                                        <button
                                            onClick={() => handleDownload(version, hash)}
                                            disabled={!selectedMirror || selectedMirror === 'manual'}
                                            className="download-button"
                                        >
                                            <FaDownload /> Download
                                        </button>
                                    )}
                                    {isDownloading && renderDownloadProgress(version, hash)}
                                    {isDownloaded && !isInstalled && (
                                        <button
                                            onClick={() => handleInstall(version, hash)}
                                            disabled={isInstalling}
                                            className="install-button"
                                        >
                                            {isInstalling ? <FaSpinner className="fa-spin" /> : <FaRocket />} Install
                                        </button>
                                    )}
                                    {isInstalled && (
                                        <FaCheck className="installed" />
                                    )}
                                </td>
                            </tr>
                        );
                    })}
                </tbody>
            </table>

            {error && (
                <div className="error-message">
                    <FaExclamationTriangle /> {error}
                </div>
            )}

            <div className="app-details">
                <div className="detail-section">
                    <h3 onClick={() => setShowMetadata(!showMetadata)}>
                        Metadata {showMetadata ? <FaChevronUp /> : <FaChevronDown />}
                    </h3>
                    {showMetadata && (
                        <pre className="json-display">
                            {JSON.stringify(app.metadata, null, 2)}
                        </pre>
                    )}
                </div>

                {manifest && (
                    <div className="detail-section">
                        <h3 onClick={() => setShowManifest(!showManifest)}>
                            Manifest {showManifest ? <FaChevronUp /> : <FaChevronDown />}
                        </h3>
                        {showManifest && (
                            <pre className="json-display">
                                {JSON.stringify(manifest, null, 2)}
                            </pre>
                        )}
                    </div>
                )}

                {manifest && manifest.request_capabilities && (
                    <div className="detail-section">
                        <h3 onClick={() => setShowCaps(!showCaps)}>
                            Capabilities {showCaps ? <FaChevronUp /> : <FaChevronDown />}
                        </h3>
                        {showCaps && (
                            <pre className="json-display">
                                {JSON.stringify(manifest.request_capabilities, null, 2)}
                            </pre>
                        )}
                    </div>
                )}
            </div>

            {showCapApproval && manifest && (
                <div className="cap-approval-popup">
                    <div className="cap-approval-content">
                        <h3>Approve Capabilities</h3>
                        <pre className="json-display">
                            {JSON.stringify(manifest[0]?.request_capabilities || [], null, 2)}
                        </pre>
                        <div className="approval-buttons">
                            <button onClick={() => setShowCapApproval(false)}>Cancel</button>
                            <button onClick={confirmInstall} disabled={isInstalling}>
                                {isInstalling ? <FaSpinner className="fa-spin" /> : 'Approve and Install'}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}