import React, { useState, useEffect } from 'react';
import classNames from 'classnames';
import { FaCheck, FaTriangleExclamation, FaCircle, FaX } from 'react-icons/fa6';

interface MessagePopupProps {
    type: 'success' | 'error' | 'info';
    content: string;
    onClose: () => void;
    autoCloseDelay?: number;
}

const MessagePopup: React.FC<MessagePopupProps> = ({ type, content, onClose, autoCloseDelay = 5000 }) => {
    const [visible, setVisible] = useState(true);

    useEffect(() => {
        const timer = setTimeout(() => {
            setVisible(false);
            onClose();
        }, 5000);


        return () => clearTimeout(timer);
    }, [onClose, autoCloseDelay]);

    if (!visible) return null;


    const icon = {
        success: <FaCheck />,
        error: <FaTriangleExclamation />,
        info: <FaCircle />
    }[type];

    return (
        <div className={classNames(
            'fixed bottom-4 right-4 p-4 rounded-lg shadow-lg flex items-center gap-4 max-w-md',
            {
                'bg-green-600': type === 'success',
                'bg-red-600': type === 'error',
                'bg-blue-600': type === 'info'
            }
        )}>
            <div className="text-2xl text-white">{icon}</div>
            <div className="flex-grow text-white">{content}</div>
            <button onClick={onClose} className="text-white hover:text-gray-200">
                <FaX />
            </button>
        </div>
    );
};

export default MessagePopup;