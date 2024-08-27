import React, { useState, useEffect, useCallback, useMemo } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { FaDownload, FaSpinner, FaChevronDown, FaChevronUp, FaRocket, FaTrash, FaPlay } from "react-icons/fa";
import useAppsStore from "../store";
import { MirrorSelector } from '../components';

export default function DownloadPage() {
    const navigate = useNavigate();
    const { id } = useParams<{ id: string }>();
    const {
        listings,
        downloads,
        installed,
        activeDownloads,
        fetchData,
        downloadApp,
        installApp,
        removeDownload,
        clearAllActiveDownloads,
    } = useAppsStore();

    const [showMetadata, setShowMetadata] = useState(false);
    const [selectedMirror, setSelectedMirror] = useState<string>("");
    const [selectedVersion, setSelectedVersion] = useState<string>("");
    const [showMyDownloads, setShowMyDownloads] = useState(false);
    const [isMirrorOnline, setIsMirrorOnline] = useState<boolean | null>(null);
    const [showCapApproval, setShowCapApproval] = useState(false);
    const [manifest, setManifest] = useState<any>(null);

    const app = useMemo(() => listings[id || ""], [listings, id]);
    const appDownloads = useMemo(() => downloads[id || ""] || [], [downloads, id]);
    const installedApp = useMemo(() => installed[id || ""], [installed, id]);

    useEffect(() => {
        if (id) {
            fetchData(id);
            clearAllActiveDownloads();
        }
    }, [id, fetchData, clearAllActiveDownloads]);

    const handleMirrorSelect = useCallback((mirror: string, status: boolean | null | 'http') => {
        setSelectedMirror(mirror);
        setIsMirrorOnline(status === 'http' ? true : status);
    }, []);


    const sortedVersions = useMemo(() => {
        if (!app || !app.metadata?.properties?.code_hashes) return [];
        return app.metadata.properties.code_hashes
            .map(([version, hash]) => ({ version, hash }))
            .sort((a, b) => {
                const vA = a.version.split('.').map(Number);
                const vB = b.version.split('.').map(Number);
                for (let i = 0; i < Math.max(vA.length, vB.length); i++) {
                    if (vA[i] > vB[i]) return -1;
                    if (vA[i] < vB[i]) return 1;
                }
                return 0;
            });
    }, [app]);

    useEffect(() => {
        if (sortedVersions.length > 0 && !selectedVersion) {
            setSelectedVersion(sortedVersions[0].version);
        }
    }, [sortedVersions, selectedVersion]);

    const isDownloaded = useMemo(() => {
        if (!app || !selectedVersion) return false;
        const versionData = sortedVersions.find(v => v.version === selectedVersion);
        if (!versionData) return false;
        return appDownloads.some(d => d.File && d.File.name === `${versionData.hash}.zip`);
    }, [app, selectedVersion, sortedVersions, appDownloads]);

    const isDownloading = useMemo(() => {
        if (!app || !selectedVersion) return false;
        const versionData = sortedVersions.find(v => v.version === selectedVersion);
        if (!versionData) return false;
        const downloadKey = `${app.package_id.package_name}:${selectedMirror}:${versionData.hash}`;
        return !!activeDownloads[downloadKey];
    }, [app, selectedVersion, sortedVersions, selectedMirror, activeDownloads]);


    const downloadProgress = useMemo(() => {
        if (!isDownloading || !app || !selectedVersion) return null;
        const versionData = sortedVersions.find(v => v.version === selectedVersion);
        if (!versionData) return null;
        const downloadKey = `${app.package_id.package_name}:${selectedMirror}:${versionData.hash}`;
        const progress = activeDownloads[downloadKey];
        return progress ? Math.round((progress.downloaded / progress.total) * 100) : 0;
    }, [isDownloading, app, selectedVersion, sortedVersions, selectedMirror, activeDownloads]);

    const isCurrentVersionInstalled = useMemo(() => {
        if (!app || !selectedVersion || !installedApp) return false;
        const versionData = sortedVersions.find(v => v.version === selectedVersion);
        return versionData ? installedApp.our_version_hash === versionData.hash : false;
    }, [app, selectedVersion, installedApp, sortedVersions]);

    const handleDownload = useCallback(() => {
        if (!id || !selectedMirror || !app || !selectedVersion) return;
        const versionData = sortedVersions.find(v => v.version === selectedVersion);
        if (versionData) {
            downloadApp(id, versionData.hash, selectedMirror);
        }
    }, [id, selectedMirror, app, selectedVersion, sortedVersions, downloadApp]);

    const handleRemoveDownload = useCallback((hash: string) => {
        if (!id) return;
        removeDownload(id, hash).then(() => fetchData(id));
    }, [id, removeDownload, fetchData]);

    const handleInstall = useCallback((version: string, hash: string) => {
        if (!id || !app) return;
        const download = appDownloads.find(d => d.File && d.File.name === `${hash}.zip`);
        if (download?.File?.manifest) {
            try {
                const manifestData = JSON.parse(download.File.manifest);
                setManifest(manifestData);
                setShowCapApproval(true);
            } catch (error) {
                console.error('Failed to parse manifest:', error);
            }
        } else {
            console.error('Manifest not found for the selected version');
        }
    }, [id, app, appDownloads]);

    const canDownload = useMemo(() => {
        return selectedMirror && (isMirrorOnline === true || selectedMirror.startsWith('http')) && !isDownloading && !isDownloaded;
    }, [selectedMirror, isMirrorOnline, isDownloading, isDownloaded]);

    const confirmInstall = useCallback(() => {
        if (!id || !selectedVersion) return;
        const versionData = sortedVersions.find(v => v.version === selectedVersion);
        if (versionData) {
            installApp(id, versionData.hash).then(() => {
                fetchData(id);
                setShowCapApproval(false);
                setManifest(null);
            });
        }
    }, [id, selectedVersion, sortedVersions, installApp, fetchData]);

    const handleLaunch = useCallback(() => {
        if (app) {
            navigate(`/${app.package_id.package_name}:${app.package_id.package_name}:${app.package_id.publisher_node}/`);
        }
    }, [app, navigate]);

    if (!app) {
        return <div className="downloads-page"><h4>Loading app details...</h4></div>;
    }

    return (
        <div className="downloads-page">
            <div className="app-header">
                <h2>{app.metadata?.name || app.package_id.package_name}</h2>
                {installedApp && (
                    <button onClick={handleLaunch} className="launch-button">
                        <FaPlay /> Launch
                    </button>
                )}
            </div>
            <p>{app.metadata?.description}</p>

            <div className="version-selector">
                <select
                    value={selectedVersion}
                    onChange={(e) => setSelectedVersion(e.target.value)}
                >
                    {sortedVersions.map(({ version }, index) => (
                        <option key={version} value={version}>
                            {version} {index === 0 ? "(newest)" : ""}
                        </option>
                    ))}
                </select>
            </div>

            <div className="download-section">
                <MirrorSelector
                    packageId={id}
                    onMirrorSelect={handleMirrorSelect}
                />
                {isCurrentVersionInstalled ? (
                    <button className="installed-button" disabled>
                        <FaRocket /> Installed
                    </button>
                ) : isDownloaded ? (
                    <button
                        onClick={() => {
                            const versionData = sortedVersions.find(v => v.version === selectedVersion);
                            if (versionData) {
                                handleInstall(versionData.version, versionData.hash);
                            }
                        }}
                        className="install-button"
                    >
                        <FaRocket /> Install
                    </button>
                ) : (
                    <button
                        onClick={handleDownload}
                        disabled={!canDownload}
                        className="download-button"
                    >
                        {isDownloading ? (
                            <>
                                <FaSpinner className="fa-spin" />
                                Downloading... {downloadProgress}%
                            </>
                        ) : (
                            <>
                                <FaDownload /> Download
                            </>
                        )}
                    </button>
                )}
            </div>

            <div className="my-downloads">
                <button onClick={() => setShowMyDownloads(!showMyDownloads)}>
                    {showMyDownloads ? <FaChevronUp /> : <FaChevronDown />} My Downloads
                </button>
                {showMyDownloads && (
                    <table>
                        <thead>
                            <tr>
                                <th>Version</th>
                                <th>Actions</th>
                            </tr>
                        </thead>
                        <tbody>
                            {appDownloads.map((download) => {
                                const fileName = download.File?.name;
                                const hash = fileName ? fileName.replace('.zip', '') : '';
                                const versionData = sortedVersions.find(v => v.hash === hash);
                                if (!versionData) return null;
                                return (
                                    <tr key={hash}>
                                        <td>{versionData.version}</td>
                                        <td>
                                            <button onClick={() => handleInstall(versionData.version, hash)}>
                                                <FaRocket /> Install
                                            </button>
                                            <button onClick={() => handleRemoveDownload(hash)}>
                                                <FaTrash /> Delete
                                            </button>
                                        </td>
                                    </tr>
                                );
                            })}
                        </tbody>
                    </table>
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
                            <button onClick={confirmInstall}>
                                Approve and Install
                            </button>
                        </div>
                    </div>
                </div>
            )}


            <div className="app-details">
                <h3>App Details</h3>
                <button onClick={() => setShowMetadata(!showMetadata)}>
                    {showMetadata ? <FaChevronUp /> : <FaChevronDown />} Metadata
                </button>
                {showMetadata && (
                    <pre>{JSON.stringify(app.metadata, null, 2)}</pre>
                )}
            </div>
        </div>
    );
}