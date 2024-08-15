import React, { useState, useEffect } from "react";
import { FaFolder, FaFile, FaChevronLeft, FaSync, FaRocket, FaSpinner, FaCheck } from "react-icons/fa";
import useAppsStore from "../store";
import { DownloadItem, PackageManifest } from "../types/Apps";

export default function MyDownloadsPage() {
    const { fetchDownloads, fetchDownloadsForApp, startMirroring, stopMirroring, installApp, getCaps, approveCaps, fetchInstalled, installed } = useAppsStore();
    const [currentPath, setCurrentPath] = useState<string[]>([]);
    const [items, setItems] = useState<DownloadItem[]>([]);
    const [isInstalling, setIsInstalling] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [showCapApproval, setShowCapApproval] = useState(false);
    const [manifest, setManifest] = useState<PackageManifest | null>(null);
    const [selectedItem, setSelectedItem] = useState<DownloadItem | null>(null);

    useEffect(() => {
        loadItems();
        fetchInstalled();
    }, [currentPath]);

    const loadItems = async () => {
        let downloads: DownloadItem[];
        if (currentPath.length === 0) {
            downloads = await fetchDownloads();
        } else {
            downloads = await fetchDownloadsForApp(currentPath.join(':'));
        }
        setItems(downloads);
    };

    const navigateToItem = (item: DownloadItem) => {
        if (item.Dir) {
            setCurrentPath([...currentPath, item.Dir.name]);
        }
    };

    const navigateUp = () => {
        setCurrentPath(currentPath.slice(0, -1));
    };

    const toggleMirroring = async (item: DownloadItem) => {
        if (item.Dir) {
            const packageId = [...currentPath, item.Dir.name].join(':');
            try {
                if (item.Dir.mirroring) {
                    await stopMirroring(packageId);
                } else {
                    await startMirroring(packageId);
                }
                await loadItems();
            } catch (error) {
                console.error("Error toggling mirroring:", error);
                setError(`Error toggling mirroring: ${error instanceof Error ? error.message : String(error)}`);
            }
        }
    };

    const handleInstall = async (item: DownloadItem) => {
        if (item.File) {
            setSelectedItem(item);
            try {
                const packageId = [...currentPath, item.File.name.replace('.zip', '')].join(':');
                const caps = await getCaps(packageId);
                setManifest(caps);
                setShowCapApproval(true);
            } catch (error) {
                console.error('Failed to get capabilities:', error);
                setError(`Failed to get capabilities: ${error instanceof Error ? error.message : String(error)}`);
            }
        }
    };

    const confirmInstall = async () => {
        if (!selectedItem?.File) return;
        setIsInstalling(true);
        setError(null);
        try {
            const packageId = [...currentPath, selectedItem.File.name.replace('.zip', '')].join(':');
            await approveCaps(packageId);
            await installApp(packageId, selectedItem.File.manifest);
            await fetchInstalled();
            setShowCapApproval(false);
            await loadItems();
        } catch (error) {
            console.error('Installation failed:', error);
            setError(`Installation failed: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setIsInstalling(false);
        }
    };

    return (
        <div className="downloads-page">
            <h2>Downloads</h2>
            <div className="file-explorer">
                <div className="path-navigation">
                    {currentPath.length > 0 && (
                        <button onClick={navigateUp} className="navigate-up">
                            <FaChevronLeft /> Back
                        </button>
                    )}
                    <span className="current-path">/{currentPath.join('/')}</span>
                </div>
                <table className="downloads-table">
                    <thead>
                        <tr>
                            <th>Name</th>
                            <th>Type</th>
                            <th>Size</th>
                            <th>Mirroring</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        {items.map((item, index) => {
                            const isFile = !!item.File;
                            const name = isFile ? item.File!.name : item.Dir!.name;
                            const isInstalled = isFile && installed.some(i => i.package_id.package_name === name.replace('.zip', ''));
                            return (
                                <tr key={index} onClick={() => navigateToItem(item)} className={isFile ? 'file' : 'directory'}>
                                    <td>
                                        {isFile ? <FaFile /> : <FaFolder />} {name}
                                    </td>
                                    <td>{isFile ? 'File' : 'Directory'}</td>
                                    <td>{isFile ? `${(item.File!.size / 1024).toFixed(2)} KB` : '-'}</td>
                                    <td>{!isFile && (item.Dir!.mirroring ? 'Yes' : 'No')}</td>
                                    <td>
                                        {!isFile && (
                                            <button onClick={(e) => { e.stopPropagation(); toggleMirroring(item); }}>
                                                <FaSync /> {item.Dir!.mirroring ? 'Stop' : 'Start'} Mirroring
                                            </button>
                                        )}
                                        {isFile && !isInstalled && (
                                            <button onClick={(e) => { e.stopPropagation(); handleInstall(item); }}>
                                                <FaRocket /> Install
                                            </button>
                                        )}
                                        {isFile && isInstalled && (
                                            <FaCheck className="installed" />
                                        )}
                                    </td>
                                </tr>
                            );
                        })}
                    </tbody>
                </table>
            </div>

            {error && (
                <div className="error-message">
                    {error}
                </div>
            )}

            {showCapApproval && manifest && (
                <div className="cap-approval-popup">
                    <h3>Approve Capabilities</h3>
                    <pre className="json-display">
                        {JSON.stringify(manifest.request_capabilities, null, 2)}
                    </pre>
                    <div className="approval-buttons">
                        <button onClick={() => setShowCapApproval(false)}>Cancel</button>
                        <button onClick={confirmInstall} disabled={isInstalling}>
                            {isInstalling ? <FaSpinner className="fa-spin" /> : 'Approve and Install'}
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}