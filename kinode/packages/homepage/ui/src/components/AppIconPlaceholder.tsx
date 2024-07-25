import React from 'react';

const AppIconPlaceholder: React.FC<{ text: string }> = ({ text }) => {
  const index = text.split('').pop()?.toUpperCase() || '0'
  const derivedFilename = `/icons/${index}`

  if (!derivedFilename) {
    return null
  }

  return <img src={derivedFilename} />
}

export default AppIconPlaceholder