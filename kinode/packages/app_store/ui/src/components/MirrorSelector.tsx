import React, { useState, useEffect } from 'react';
import useAppsStore from "../store";

interface MirrorSelectorProps {
    packageId: string | undefined;
    onMirrorSelect: (mirror: string) => void;
}

const MirrorSelector: React.FC<MirrorSelectorProps> = ({ packageId, onMirrorSelect }) => {
    const { fetchListing, checkMirror } = useAppsStore();
    const [selectedMirror, setSelectedMirror] = useState<string>("");
    const [customMirror, setCustomMirror] = useState<string>("");
    const [isCustomMirrorSelected, setIsCustomMirrorSelected] = useState(false);
    const [mirrorStatuses, setMirrorStatuses] = useState<{ [mirror: string]: boolean | null | 'http' }>({});
    const [availableMirrors, setAvailableMirrors] = useState<string[]>([]);

    useEffect(() => {
        const fetchMirrors = async () => {
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
                    checkMirror(mirror)
                        .then(status => setMirrorStatuses(prev => ({ ...prev, [mirror]: status?.is_online ?? false })))
                        .catch(() => setMirrorStatuses(prev => ({ ...prev, [mirror]: false })));
                }
            });
        };

        fetchMirrors();
    }, [packageId, fetchListing, checkMirror]);

    useEffect(() => {
        onMirrorSelect(selectedMirror);
    }, [selectedMirror, onMirrorSelect]);

    const handleMirrorChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
        const value = e.target.value;
        if (value === "custom") {
            setIsCustomMirrorSelected(true);
        } else {
            setSelectedMirror(value);
            setIsCustomMirrorSelected(false);
            setCustomMirror("");
        }
    };

    const handleSetCustomMirror = () => {
        if (customMirror) {
            setSelectedMirror(customMirror);
            setIsCustomMirrorSelected(false);
            setAvailableMirrors(prev => [...prev, customMirror]);

            if (customMirror.startsWith('http')) {
                setMirrorStatuses(prev => ({ ...prev, [customMirror]: 'http' }));
            } else {
                checkMirror(customMirror)
                    .then(status => setMirrorStatuses(prev => ({ ...prev, [customMirror]: status?.is_online ?? false })))
                    .catch(() => setMirrorStatuses(prev => ({ ...prev, [customMirror]: false })));
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
                    <option key={`${mirror}-${index}`} value={mirror} disabled={mirrorStatuses[mirror] === false}>
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