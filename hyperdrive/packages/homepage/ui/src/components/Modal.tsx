import { FaX } from "react-icons/fa6"
import { useEffect } from 'react';

interface Props extends React.HTMLAttributes<HTMLDivElement> {
  title: string
  onClose: () => void
}

export const Modal: React.FC<Props> = ({ title, onClose, children }) => {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [onClose]);

  return (
    <div className="modal">
      <div className="modal-inner">
        <div className="modal-header">
          <h1>{title}</h1>
          <button className="secondary" onClick={onClose}>
            <FaX />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}