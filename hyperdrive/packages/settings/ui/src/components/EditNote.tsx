import React, { useState } from 'react';
import { useAccount, useSendTransaction } from 'wagmi';
import { useConnectModal, useAddRecentTransaction } from '@rainbow-me/rainbowkit';
import { noteFunction } from '../abis/helpers';
import { HYPERMAP, mechAbi } from '../abis';
import { encodeFunctionData } from 'viem';

interface EditNoteProps {
    label?: string;
    tba: string;
    field_placeholder: string;
}

const EditNote: React.FC<EditNoteProps> = ({ label: initialLabel, tba, field_placeholder }) => {
    const [value, setValue] = useState('');
    // Ensure initial label has tilde
    const [label, setLabel] = useState(initialLabel ? (initialLabel.startsWith('~') ? initialLabel : `~${initialLabel}`) : '~');
    const editMode = !initialLabel;

    const { address } = useAccount();
    const { openConnectModal } = useConnectModal();
    const addRecentTransaction = useAddRecentTransaction();

    const { sendTransaction, isPending } = useSendTransaction({
        mutation: {
            onSuccess: (tx_hash) => {
                addRecentTransaction({ hash: tx_hash, description: `adding note ${label}` });
            },
            onSettled: () => {
            },
        }
    });

    const handleAddNote = async () => {
        if (!address) {
            openConnectModal?.();
            return;
        }

        if (!label) {
            return;
        }

        const txn = encodeFunctionData({
            abi: mechAbi,
            functionName: 'execute',
            args: [
                HYPERMAP,
                BigInt(0),
                noteFunction(label, value),
                0,
            ],
        });
        console.log(txn);
        sendTransaction({
            to: tba as `0x${string}`,
            data: txn,
            gas: BigInt(1000000),
        });

    };

    return (
        <div className="edit-note">
            {editMode && <input
                type="text"
                placeholder="label"
                value={label}
                onChange={(e) => {
                    // Ensure tilde is always present
                    const newValue = e.target.value;
                    if (!newValue.startsWith('~')) {
                        setLabel(`~${newValue}`);
                    } else {
                        setLabel(newValue);
                    }
                }}
                className="note-input"
                style={{ minWidth: '200px' }}
            />}
            <input type="text" placeholder={field_placeholder} value={value} onChange={(e) => setValue(e.target.value)} className="note-input" style={{ minWidth: '400px' }} />
            <button
                onClick={handleAddNote}
                className={`add-note-button ${isPending ? 'loading' : ''}`}
                disabled={isPending}
            >Submit</button>

        </div>
    );

};

export default EditNote;