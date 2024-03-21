import React from 'react';
import { FaEllipsisH } from 'react-icons/fa';
import { Menu, MenuButton } from '@szhsin/react-menu';

interface DropdownProps extends React.HTMLAttributes<HTMLDivElement> {
}

export default function Dropdown({ ...props }: DropdownProps) {
  return (
    <Menu {...props} className={"dropdown " + props.className} menuButton={<MenuButton className="small">
      <FaEllipsisH style={{ marginBottom: '-0.125em' }} />
    </MenuButton>}>
      {props.children}
    </Menu>
  )
}
