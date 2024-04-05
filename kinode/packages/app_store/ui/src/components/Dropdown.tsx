import React from 'react';
import { FaEllipsisH } from 'react-icons/fa';
import { Menu, MenuButton } from '@szhsin/react-menu';

interface DropdownProps extends React.HTMLAttributes<HTMLDivElement> {
}

export default function Dropdown({ ...props }: DropdownProps) {
  return (
    <Menu
      {...props}
      className={"cursor-pointer relative" + props.className}
      menuButton={<MenuButton className="small">
        <FaEllipsisH className='-mb-1' />
      </MenuButton>}
    >
      {props.children}
    </Menu>
  )
}
