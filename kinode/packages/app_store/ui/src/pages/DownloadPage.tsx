import React, { useState, useEffect } from "react";
import { useParams } from "react-router-dom";
import { FaDownload, FaCheck, FaSpinner, FaRocket, FaChevronDown, FaChevronUp, FaExclamationTriangle, FaTrash } from "react-icons/fa";
import useAppsStore from "../store";
import { DownloadItem, AppListing } from "../types/Apps";
import { MirrorSelector } from '../components';


export default function DownloadPage() {
    const { id } = useParams<{ id: string }>();
    const {
        fetchListing,
        fetchDownloadsForApp,
        downloadApp,
        installApp,
        removeDownload,
        fetchInstalledApp,
        activeDownloads
    } = useAppsStore();

    const [app, setApp] = useState<AppListing | null>(null);
    const [downloads, setDownloads] = useState<DownloadItem[]>([]);
    const [installedApp, setInstalledApp] = useState<any | null>(null);
    const [selectedMirror, setSelectedMirror] = useState<string>("");
    const [error, setError] = useState<string | null>(null);
    const [showMetadata, setShowMetadata] = useState(false);

    useEffect(() => {
        if (!id) return;

        const fetchData = async () => {
            try {
                const appData = await fetchListing(id);
                setApp(appData);
                setSelectedMirror(appData.package_id.publisher_node);

                const downloadsData = await fetchDownloadsForApp(id);
                setDownloads(downloadsData);

                const installedAppData = await fetchInstalledApp(id);
                setInstalledApp(installedAppData);
            } catch (error) {
                console.error("Error fetching app data:", error);
                setError(`Error fetching app data: ${error instanceof Error ? error.message : String(error)}`);
            }
        };

        fetchData();
    }, [id, fetchListing, fetchDownloadsForApp, fetchInstalledApp]);


    const handleDownload = async (version: string, hash: string) => {
        if (!id || !selectedMirror) return;
        setError(null);
        try {
            await downloadApp(id, hash, selectedMirror);
            const updatedDownloads = await fetchDownloadsForApp(id);
            setDownloads(updatedDownloads);
        } catch (error) {
            setError(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
        }
    };

    const handleInstall = async (version: string, hash: string) => {
        if (!id) return;
        setError(null);
        try {
            await installApp(id, hash);
            const updatedInstalledApp = await fetchInstalledApp(id);
            setInstalledApp(updatedInstalledApp);
        } catch (error) {
            setError(`Installation failed: ${error instanceof Error ? error.message : String(error)}`);
        }
    };

    const handleRemoveDownload = async (version: string, hash: string) => {
        if (!id) return;
        try {
            await removeDownload(id, hash);
            const updatedDownloads = await fetchDownloadsForApp(id);
            setDownloads(updatedDownloads);
        } catch (error) {
            setError(`Failed to remove download: ${error instanceof Error ? error.message : String(error)}`);
        }
    };

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
                <table>
                    <thead>
                        <tr>
                            <th>Version</th>
                            <th>Status</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        {app.metadata?.properties?.code_hashes.map(([version, hash]) => {
                            const download = downloads.find(d => d.File && d.File.name === `${hash}.zip`);
                            const isDownloaded = !!download;
                            const isInstalled = installedApp?.our_version_hash === hash;
                            const downloadKey = `${app.package_id.package_name}:${app.package_id.publisher_node}:${hash}`;
                            const isDownloading = !!activeDownloads[downloadKey];

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
                                                disabled={!selectedMirror}
                                                className="download-button"
                                            >
                                                <FaDownload /> Download
                                            </button>
                                        )}
                                        {isDownloading && <FaSpinner className="fa-spin" />}
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
                            );
                        })}
                    </tbody>
                </table>
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

            {error && (
                <div className="error-message">
                    <FaExclamationTriangle /> {error}
                </div>
            )}
        </div>
    );
}