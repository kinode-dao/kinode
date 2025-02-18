import React from 'react';

interface TooltipProps {
  content: React.ReactNode;
  children?: React.ReactNode;
}

export function Tooltip({ content, children }: TooltipProps) {
  return (
    <div className="tooltip-container">
      {children}
      <span className="tooltip-icon">ⓘ</span>
      <div className="tooltip-content">{content}</div>
    </div>
  );
}
