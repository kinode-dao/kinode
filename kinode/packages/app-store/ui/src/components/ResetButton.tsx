import React, { useState } from 'react';
import { FaExclamationTriangle } from 'react-icons/fa';
import useAppsStore from '../store';

const ResetButton: React.FC = () => {
    const resetStore = useAppsStore(state => state.resetStore);
    const [isOpen, setIsOpen] = useState(false);
    const [isLoading, setIsLoading] = useState(false);

    const handleReset = async () => {
        try {
            setIsLoading(true);
            await resetStore();
            setIsOpen(false);
        } catch (error) {
            console.error('Reset failed:', error);
            alert('Failed to reset the app store. Please try again.');
        } finally {
            setIsLoading(false);
        }
    };

    return (
        <>
            <button
                onClick={() => setIsOpen(true)}
                className="button danger"
                style={{ fontSize: '0.9rem' }}
            >
                Reset Store
            </button>

            {isOpen && (
                <div className="modal-overlay" onClick={() => setIsOpen(false)}>
                    <div className="modal-content" onClick={e => e.stopPropagation()}>
                        <button className="modal-close" onClick={() => setIsOpen(false)}>×</button>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem', marginBottom: '1rem' }}>
                            <FaExclamationTriangle size={24} style={{ color: 'var(--red)' }} />
                            <h3 style={{ margin: 0 }}>Warning</h3>
                        </div>

                        <p style={{ marginBottom: '1.5rem' }}>
                            This action will re-index all apps and reset the store state. 
                            Only proceed if you know what you're doing.
                        </p>

                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '0.75rem' }}>
                            <button
                                onClick={() => setIsOpen(false)}
                                className="button"
                            >
                                Cancel
                            </button>
                            <button
                                onClick={handleReset}
                                disabled={isLoading}
                                className="button danger"
                            >
                                {isLoading ? 'Resetting...' : 'Reset Store'}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </>
    );
};

export default ResetButton;
