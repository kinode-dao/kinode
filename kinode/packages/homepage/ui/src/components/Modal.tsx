import { FaX } from "react-icons/fa6"

interface Props extends React.HTMLAttributes<HTMLDivElement> {
  title: string
  onClose: () => void
}

export const Modal: React.FC<Props> = ({ title, onClose, children }) => {
  return (
    <div>
      <div>
        <div>
          <h1>{title}</h1>
          <button onClick={onClose}>
            <FaX />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}