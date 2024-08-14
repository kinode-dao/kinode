import React, { useState, useEffect } from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaSpinner, FaRocket, FaChevronDown, FaChevronUp, FaExclamationTriangle } from "react-icons/fa";
import useAppsStore from "../store";
import { AppListing, DownloadItem, MirrorCheckFile, PackageManifest } from "../types/Apps";

export default function DownloadPage() {
    const { id } = useParams();
    const { listings, fetchListings, fetchDownloadsForApp, downloadApp, installApp, checkMirror, fetchInstalled, installed, getCaps } = useAppsStore();
    const [downloads, setDownloads] = useState<DownloadItem[]>([]);
    const [selectedMirror, setSelectedMirror] = useState<string | null>(null);
    const [mirrorStatuses, setMirrorStatuses] = useState<{ [mirror: string]: MirrorCheckFile | null }>({});
    const [isDownloading, setIsDownloading] = useState(false);
    const [isInstalling, setIsInstalling] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [showMetadata, setShowMetadata] = useState(false);
    const [showManifest, setShowManifest] = useState(false);
    const [showCaps, setShowCaps] = useState(false);
    const [manifest, setManifest] = useState<PackageManifest | null>(null);

    const app = listings.find(a => `${a.package_id.package_name}:${a.package_id.publisher_node}` === id);

    useEffect(() => {
        fetchListings();
        fetchInstalled();
        if (id) {
            fetchDownloadsForApp(id).then(setDownloads);
        }
    }, [id, fetchListings, fetchDownloadsForApp, fetchInstalled]);

    useEffect(() => {
        if (app) {
            checkMirrors();
        }
    }, [app]);

    const checkMirrors = async () => {
        if (!app) return;
        const mirrors = [app.package_id.publisher_node, ...(app.metadata?.properties?.mirrors || [])];
        const statuses: { [mirror: string]: MirrorCheckFile | null } = {};
        for (const mirror of mirrors) {
            const status = await checkMirror(mirror);
            statuses[mirror] = status;
        }
        setMirrorStatuses(statuses);
        setSelectedMirror(statuses[app.package_id.publisher_node]?.is_online ? app.package_id.publisher_node : mirrors.find(m => statuses[m]?.is_online) || null);
    };

    const handleDownload = async (version: string) => {
        if (!app || !selectedMirror) return;
        setIsDownloading(true);
        setError(null);
        try {
            await downloadApp(`${app.package_id.package_name}:${app.package_id.publisher_node}`, selectedMirror, version);
            const updatedDownloads = await fetchDownloadsForApp(id!);
            setDownloads(updatedDownloads);
        } catch (error) {
            console.error('Download failed:', error);
            setError(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsDownloading(false);
        }
    };

    const handleInstall = async (version: string) => {
        if (!app) return;
        setIsInstalling(true);
        setError(null);
        try {
            await installApp(`${app.package_id.package_name}:${app.package_id.publisher_node}`, version);
            fetchInstalled();
        } catch (error) {
            console.error('Installation failed:', error);
            setError(`Installation failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsInstalling(false);
        }
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
                    value={selectedMirror || ''}
                    onChange={(e) => setSelectedMirror(e.target.value)}
                    disabled={isDownloading}
                >
                    <option value="" disabled>Select Mirror</option>
                    {Object.entries(mirrorStatuses).map(([mirror, status]) => (
                        <option key={mirror} value={mirror} disabled={!status?.is_online}>
                            {mirror} {status?.is_online ? '(Online)' : '(Offline)'}
                        </option>
                    ))}
                </select>
            </div>

            <h3>Available Versions</h3>
            <table className="downloads-table">
                <thead>
                    <tr>
                        <th>Version</th>
                        <th>Size</th>
                        <th>Status</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {app.metadata?.properties?.code_hashes.map(([version, hash]) => {
                        const download = downloads.find(d => d.name === `${hash}.zip`);
                        const isDownloaded = !!download;
                        const isInstalled = installed.some(i => i.package_id.package_name === app.package_id.package_name && i.our_version_hash === version);

                        return (
                            <tr key={version}>
                                <td>{version}</td>
                                <td>{download?.size ? `${(download.size / 1024 / 1024).toFixed(2)} MB` : 'N/A'}</td>
                                <td>
                                    {isInstalled ? 'Installed' : isDownloaded ? 'Downloaded' : 'Not downloaded'}
                                </td>
                                <td>
                                    {!isDownloaded && (
                                        <button
                                            onClick={() => handleDownload(version)}
                                            disabled={!selectedMirror || isDownloading}
                                            className="download-button"
                                        >
                                            {isDownloading ? <FaSpinner className="fa-spin" /> : <FaDownload />} Download
                                        </button>
                                    )}
                                    {isDownloaded && !isInstalled && (
                                        <button
                                            onClick={() => handleInstall(version)}
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
        </div>
    );
}