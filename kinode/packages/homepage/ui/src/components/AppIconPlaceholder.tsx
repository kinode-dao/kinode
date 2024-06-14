import React from 'react';

import { isMobileCheck } from '../utils/dimensions';
import classNames from 'classnames';

const AppIconPlaceholder: React.FC<{ text: string, className?: string, size: 'small' | 'medium' | 'large' }> = ({ text, className, size }) => {
  const index = text.split('').pop()?.toUpperCase() || '0'
  const derivedFilename = `/api/icons/${index}`

  if (!derivedFilename) {
    return null
  }

  const isMobile = isMobileCheck()

  return <img
    src={derivedFilename}
    className={classNames('m-0 align-self-center rounded-full', {
      'h-32 w-32': !isMobile && size === 'large',
      'h-18 w-18': !isMobile && size === 'medium',
      'h-12 w-12': isMobile || size === 'small',
    }, className)}
  />
}

export default AppIconPlaceholder