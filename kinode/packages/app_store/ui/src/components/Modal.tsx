import classNames from 'classnames'
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
    <div
      className={classNames(`bg-black/25 fixed top-0 bottom-0 left-0 right-0 flex flex-col c z-30 min-h-[10em] min-w-[20em]`,
        { show }
      )}
      onClick={hide}
    >
      <div
        {...props}
        className={`flex flex-col relative bg-black/90 rounded-lg p-6 ${props.className || ''}`}
        onClick={dontHide}
      >
        {Boolean(title) && <h4 className='mt-0 mb-2'>{title}</h4>}
        {!hideClose && (
          <button
            className='icon'
            onClick={hide}
          >
            <FaPlus />
          </button>
        )}
        <div
          className='flex flex-col items-center w-full'
          onClick={dontHide}
        >
          {props.children}
        </div>
      </div>
    </div>
  )
}

export default Modal