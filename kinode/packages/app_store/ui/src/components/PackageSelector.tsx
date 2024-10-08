import React, { useState, useEffect } from 'react';
import useAppsStore from "../store";

interface PackageSelectorProps {
    onPackageSelect: (packageName: string, publisherId: string) => void;
}

const PackageSelector: React.FC<PackageSelectorProps> = ({ onPackageSelect }) => {
    const { installed, fetchInstalled } = useAppsStore();
    const [selectedPackage, setSelectedPackage] = useState<string>("");
    const [customPackage, setCustomPackage] = useState<string>("");
    const [isCustomPackageSelected, setIsCustomPackageSelected] = useState(false);

    useEffect(() => {
        if (selectedPackage && selectedPackage !== "custom") {
            const [packageName, publisherId] = selectedPackage.split(':');
            onPackageSelect(packageName, publisherId);
        }
    }, [selectedPackage, onPackageSelect]);

    useEffect(() => {
        fetchInstalled();
    }, []);

    const handlePackageChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
        const value = e.target.value;
        if (value === "custom") {
            setIsCustomPackageSelected(true);
        } else {
            setSelectedPackage(value);
            setIsCustomPackageSelected(false);
            setCustomPackage("");
        }
    };

    const handleSetCustomPackage = () => {
        if (customPackage) {
            const [packageName, publisherId] = customPackage.split(':');
            if (packageName && publisherId) {
                onPackageSelect(packageName, publisherId);
                setSelectedPackage(customPackage);
                setIsCustomPackageSelected(false);
            } else {
                alert("Please enter the package name and publisher ID in the format 'packageName:publisherId'");
            }
        }
    };

    return (
        <div className="package-selector">
            <select value={selectedPackage} onChange={handlePackageChange}>
                <option value="">Select a package</option>
                <option value="custom">Custom package</option>
                {!isCustomPackageSelected && customPackage && (
                    <option value={customPackage}>{customPackage}</option>
                )}
                {Object.keys(installed).map((packageFullName) => (
                    <option key={packageFullName} value={packageFullName}>
                        {packageFullName}
                    </option>
                ))}
            </select>
            {isCustomPackageSelected && (
                <div className="custom-package-input">
                    <input
                        type="text"
                        value={customPackage}
                        onChange={(e) => setCustomPackage(e.target.value)}
                        placeholder="Enter as packageName:publisherId"
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <button onClick={handleSetCustomPackage} disabled={!customPackage}>
                        Set Custom Package
                    </button>
                </div>
            )}
        </div>
    );
};

export default PackageSelector;