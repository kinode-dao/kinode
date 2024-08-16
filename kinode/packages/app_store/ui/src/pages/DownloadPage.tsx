import React, { useState, useEffect, useCallback, useMemo } from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaSpinner, FaRocket, FaChevronDown, FaChevronUp, FaTrash } from "react-icons/fa";
import useAppsStore from "../store";
import { MirrorSelector } from '../components';

export default function DownloadPage() {
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

    const app = useMemo(() => listings[id || ""], [listings, id]);
    const appDownloads = useMemo(() => downloads[id || ""] || [], [downloads, id]);
    const installedApp = useMemo(() => installed[id || ""], [installed, id]);

    useEffect(() => {
        if (id) {
            clearAllActiveDownloads();
            fetchData(id);
        }
    }, [id, fetchData, clearAllActiveDownloads]);

    useEffect(() => {
        if (app && !selectedMirror) {
            setSelectedMirror(app.package_id.publisher_node || "");
        }
    }, [app, selectedMirror]);

    const handleDownload = useCallback((version: string, hash: string) => {
        if (!id || !selectedMirror || !app) return;
        downloadApp(id, hash, selectedMirror);
    }, [id, selectedMirror, app, downloadApp]);

    const handleInstall = useCallback((version: string, hash: string) => {
        if (!id) return;
        installApp(id, hash).then(() => fetchData(id));
    }, [id, installApp, fetchData]);

    const handleRemoveDownload = useCallback((version: string, hash: string) => {
        if (!id) return;
        removeDownload(id, hash).then(() => fetchData(id));
    }, [id, removeDownload, fetchData]);

    const versionList = useMemo(() => {
        if (!app || !app.metadata?.properties?.code_hashes) return [];

        return app.metadata.properties.code_hashes.map(([version, hash]) => {
            const download = appDownloads.find(d => d.File && d.File.name === `${hash}.zip`);
            const downloadKey = `${app.package_id.package_name}:${app.package_id.publisher_node}:${hash}`;
            const activeDownload = activeDownloads[downloadKey];
            const isDownloaded = !!download?.File && download.File.size > 0;
            const isInstalled = installedApp?.our_version_hash === hash;
            const isDownloading = !!activeDownload && activeDownload.downloaded < activeDownload.total;
            const progress = isDownloading ? activeDownload : { downloaded: 0, total: 100 };

            console.log(`Version ${version} - isInstalled: ${isInstalled}, installedApp:`, installedApp);

            return { version, hash, isDownloaded, isInstalled, isDownloading, progress };
        });
    }, [app, appDownloads, activeDownloads, installedApp]);

    if (!app) {
        return <div className="downloads-page"><h4>Loading app details...</h4></div>;
    }

    return (
        <div className="downloads-page">
            <h2>{app.metadata?.name || app.package_id.package_name}</h2>
            <p>{app.metadata?.description}</p>

            <MirrorSelector packageId={id} onMirrorSelect={setSelectedMirror} />

            <div className="version-list">
                <h3>Available Versions</h3>
                {versionList.length === 0 ? (
                    <p>No versions available for this app.</p>
                ) : (
                    <table>
                        <thead>
                            <tr>
                                <th>Version</th>
                                <th>Status</th>
                                <th>Actions</th>
                            </tr>
                        </thead>
                        <tbody>
                            {versionList.map(({ version, hash, isDownloaded, isInstalled, isDownloading, progress }) => (
                                <tr key={version}>
                                    <td>{version}</td>
                                    <td>
                                        {isInstalled ? 'Installed' : isDownloaded ? 'Downloaded' : isDownloading ? 'Downloading' : 'Not downloaded'}
                                    </td>
                                    <td>
                                        {!isDownloaded && !isDownloading && (
                                            <button
                                                onClick={() => handleDownload(version, hash)}
                                                disabled={!selectedMirror}
                                                className="download-button"
                                            >
                                                <FaDownload /> Download
                                            </button>
                                        )}
                                        {isDownloading && (
                                            <div className="download-progress">
                                                <FaSpinner className="fa-spin" />
                                                Downloading... {Math.round((progress.downloaded / progress.total) * 100)}%
                                            </div>
                                        )}
                                        {isDownloaded && !isInstalled && (
                                            <>
                                                <button
                                                    onClick={() => handleInstall(version, hash)}
                                                    className="install-button"
                                                >
                                                    <FaRocket /> Install
                                                </button>
                                                <button
                                                    onClick={() => handleRemoveDownload(version, hash)}
                                                    className="delete-button"
                                                >
                                                    <FaTrash /> Delete
                                                </button>
                                            </>
                                        )}
                                        {isInstalled && <FaCheck className="installed" />}
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </table>
                )}
            </div>

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