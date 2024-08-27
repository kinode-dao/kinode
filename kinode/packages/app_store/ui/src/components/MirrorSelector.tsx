import React, { useState, useEffect, useCallback } from 'react';
import useAppsStore from "../store";

interface MirrorSelectorProps {
    packageId: string | undefined;
    onMirrorSelect: (mirror: string, status: boolean | null | 'http') => void;
}

const MirrorSelector: React.FC<MirrorSelectorProps> = ({ packageId, onMirrorSelect }) => {
    const { fetchListing, checkMirror } = useAppsStore();
    const [selectedMirror, setSelectedMirror] = useState<string>("");
    const [customMirror, setCustomMirror] = useState<string>("");
    const [isCustomMirrorSelected, setIsCustomMirrorSelected] = useState(false);
    const [mirrorStatuses, setMirrorStatuses] = useState<{ [mirror: string]: boolean | null | 'http' }>({});
    const [availableMirrors, setAvailableMirrors] = useState<string[]>([]);

    const fetchMirrors = useCallback(async () => {
        if (!packageId) return;

        const appData = await fetchListing(packageId);
        if (!appData) return;
        const mirrors = [appData.package_id.publisher_node, ...(appData.metadata?.properties?.mirrors || [])];
        setAvailableMirrors(mirrors);
        setSelectedMirror(appData.package_id.publisher_node);

        mirrors.forEach(mirror => {
            if (mirror.startsWith('http')) {
                setMirrorStatuses(prev => ({ ...prev, [mirror]: 'http' }));
            } else {
                setMirrorStatuses(prev => ({ ...prev, [mirror]: null }));
                checkMirrorStatus(mirror);
            }
        });
    }, [packageId, fetchListing, checkMirror]);

    useEffect(() => {
        fetchMirrors();
    }, [fetchMirrors]);

    const checkMirrorStatus = useCallback(async (mirror: string) => {
        try {
            const status = await checkMirror(mirror);
            setMirrorStatuses(prev => ({ ...prev, [mirror]: status?.is_online ?? false }));
        } catch {
            setMirrorStatuses(prev => ({ ...prev, [mirror]: false }));
        }
    }, [checkMirror]);

    useEffect(() => {
        if (selectedMirror) {
            const status = mirrorStatuses[selectedMirror];
            onMirrorSelect(selectedMirror, status);
        }
    }, [selectedMirror, mirrorStatuses, onMirrorSelect]);

    const handleMirrorChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
        const value = e.target.value;
        if (value === "custom") {
            setIsCustomMirrorSelected(true);
        } else {
            setSelectedMirror(value);
            setIsCustomMirrorSelected(false);
            setCustomMirror("");
            if (!value.startsWith('http')) {
                // Recheck the status when a non-HTTP mirror is selected
                setMirrorStatuses(prev => ({ ...prev, [value]: null }));
                await checkMirrorStatus(value);
            }
        }
    };

    const handleSetCustomMirror = async () => {
        if (customMirror) {
            setSelectedMirror(customMirror);
            setIsCustomMirrorSelected(false);
            setAvailableMirrors(prev => [...prev, customMirror]);

            if (customMirror.startsWith('http')) {
                setMirrorStatuses(prev => ({ ...prev, [customMirror]: 'http' }));
            } else {
                await checkMirrorStatus(customMirror);
            }
        }
    };

    const getMirrorStatus = (mirror: string, status: boolean | null | 'http') => {
        if (status === 'http') return '(HTTP)';
        if (status === null) return '(checking)';
        return status ? '(online)' : '(offline)';
    };

    return (
        <div className="mirror-selector">
            <select value={selectedMirror || ""} onChange={handleMirrorChange}>
                <option value="">Select a mirror</option>
                {availableMirrors.map((mirror, index) => (
                    <option key={`${mirror}-${index}`} value={mirror}>
                        {mirror} {getMirrorStatus(mirror, mirrorStatuses[mirror])}
                    </option>
                ))}
                <option value="custom">Custom mirror</option>
            </select>
            {isCustomMirrorSelected && (
                <div className="custom-mirror-input">
                    <input
                        type="text"
                        value={customMirror}
                        onChange={(e) => setCustomMirror(e.target.value)}
                        placeholder="Enter custom mirror URL"
                    />
                    <button onClick={handleSetCustomMirror} disabled={!customMirror}>
                        Set Custom Mirror
                    </button>
                </div>
            )}
        </div>
    );
};

export default MirrorSelector;