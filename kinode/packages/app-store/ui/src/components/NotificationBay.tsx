import React, { ReactNode, useState } from 'react';
import { FaBell, FaChevronDown, FaChevronUp, FaTrash, FaTimes } from 'react-icons/fa';
import useAppsStore from '../store';
import { Notification, NotificationAction } from '../types/Apps';
import { useNavigate } from 'react-router-dom';


interface ModalProps {
    children: ReactNode;
    onClose: () => void;
}

const Modal: React.FC<ModalProps> = ({ children, onClose }) => {
    return (
        <div className="modal-overlay">
            <div className="modal-content">
                <button className="modal-close" onClick={onClose}>
                    <FaTimes />
                </button>
                {children}
            </div>
        </div>
    );
};

const NotificationBay: React.FC = () => {
    const { notifications, removeNotification } = useAppsStore();
    const hasErrors = notifications.some(n => n.type === 'error');
    const [isExpanded, setIsExpanded] = useState(false);
    const [modalContent, setModalContent] = useState<React.ReactNode | null>(null);
    const navigate = useNavigate();

    const handleActionClick = (action: NotificationAction) => {
        switch (action.action.type) {
            case 'modal':
                const content = typeof action.action.modalContent === 'function'
                    ? action.action.modalContent()
                    : action.action.modalContent;
                setModalContent(content);
                break;
            case 'click':
                action.action.onClick?.();
                break;
            case 'redirect':
                if (action.action.path) {
                    navigate(action.action.path);
                }
                break;
        }
    };

    const handleDismiss = (notificationId: string, event: React.MouseEvent) => {
        event.stopPropagation(); // Prevent event bubbling
        removeNotification(notificationId);
    };

    const renderNotification = (notification: Notification) => {
        return (
            <div key={notification.id} className={`notification-item ${notification.type}`}>
                {notification.renderContent ? (
                    notification.renderContent(notification)
                ) : (
                    <>
                        <div className="notification-content">
                            <p>{notification.message}</p>
                            {notification.type === 'download' && notification.metadata?.progress && (
                                <div className="progress-bar">
                                    <div
                                        className="progress"
                                        style={{ width: `${notification.metadata.progress}%` }}
                                    />
                                </div>
                            )}
                        </div>

                        {notification.actions && (
                            <div className="notification-actions">
                                {notification.actions.map((action, index) => (
                                    <button
                                        key={index}
                                        onClick={() => handleActionClick(action)}
                                        className={`action-button ${action.variant || 'secondary'}`}
                                    >
                                        {action.icon && <action.icon />}
                                        {action.label}
                                    </button>
                                ))}
                            </div>
                        )}

                        {!notification.persistent && (
                            <button
                                className="dismiss-button"
                                onClick={(e) => handleDismiss(notification.id, e)}
                            >
                                <FaTrash />
                            </button>
                        )}
                    </>
                )}
            </div>
        );
    };

    return (
        <>
            <div className="notification-bay">
                <button
                    onClick={() => setIsExpanded(!isExpanded)}
                    className={`notification-button ${hasErrors ? 'has-errors' : ''}`}
                >
                    <FaBell />
                    {notifications.length > 0 && (
                        <span className={`badge ${hasErrors ? 'error-badge' : ''}`}>
                            {notifications.length}
                        </span>
                    )}
                </button>

                {isExpanded && (
                    <div className="notification-details">
                        {notifications.length === 0 ? (
                            <p>All clear, no notifications!</p>
                        ) : (
                            notifications.map(renderNotification)
                        )}
                    </div>
                )}
            </div>

            {modalContent && (
                <Modal onClose={() => setModalContent(null)}>
                    {modalContent}
                </Modal>
            )}
        </>
    );
};

export default NotificationBay;