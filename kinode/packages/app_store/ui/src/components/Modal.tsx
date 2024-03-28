import React, { MouseEvent } from 'react'
import { FaPlus } from 'react-icons/fa'

export interface ModalProps extends React.HTMLAttributes<HTMLDivElement> {
  show: boolean
  hide: () => void
  hideClose?: boolean
  children: React.ReactNode,
  title?: string
}

const Modal: React.FC<ModalProps> = ({
  show,
  hide,
  hideClose = false,
  title,
  ...props
}) => {
  const dontHide = (e: MouseEvent) => {
    e.stopPropagation()
  }

  if (!show) {
    return null
  }

  return (
    <div className={`modal-backdrop ${show ? 'show' : ''}`} onClick={hide}>
      <div {...props} className={`col modal ${props.className || ''}`} onClick={dontHide}>
        {Boolean(title) && <h4 className='modal-title'>{title}</h4>}
        {!hideClose && (
          <FaPlus className='close' onClick={hide} />
        )}
        <div className='col modal-content' onClick={dontHide}>
          {props.children}
        </div>
      </div>
    </div>
  )
}

export default Modal