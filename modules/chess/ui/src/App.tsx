import { FormEvent, useCallback, useEffect, useMemo, useState, MouseEvent } from 'react'
import { Chess } from "chess.js";
import { Chessboard } from "react-chessboard";
import UqbarEncryptorApi from '@uqbar/client-encryptor-api'
import useChessStore, { Game } from './store';

declare global {
  var window: Window & typeof globalThis;
  var our: { node: string, process: string };
}

import './App.css'

let inited = false

interface SelectedGame extends Game {
  game: Chess
}

const isTurn = (game: Game, node: string) => (game.turns || 0) % 2 === 0 ? node === game.white : node === game.black

const BASE_URL = import.meta.env.BASE_URL;
if (window.our) window.our.process = BASE_URL?.replace("/", "");

const PROXY_TARGET = `${(import.meta.env.VITE_NODE_URL || "http://localhost:8080")}${BASE_URL}`;

// This env also has BASE_URL which should match the process + package name
const WEBSOCKET_URL = import.meta.env.DEV
  ? `${PROXY_TARGET.replace('http', 'ws')}`
  : undefined;

function App() {
  const { games, handleWsMessage, set } = useChessStore()
  const [screen, setScreen] = useState('new')
  const [newGame, setNewGame] = useState('')

  const game: SelectedGame | undefined = useMemo(() => games[screen] ? ({ ...games[screen], game: new Chess(games[screen].board) }) : undefined, [games, screen])
  const currentTurn = useMemo(() => (game?.turns || 0) % 2 === 0 ? `${game?.white} (white)` : `${game?.black} (black)`, [game])

  useEffect(() => {
    if (!inited) {
      inited = true

      new UqbarEncryptorApi({
        uri: WEBSOCKET_URL,
        nodeId: window.our.node,
        processId: window.our.process,
        onMessage: handleWsMessage
      });
    }

    fetch(`${BASE_URL}/games`).then(res => res.json()).then((games) => {
      set({ games })
    }).catch(console.error)

  }, []) // eslint-disable-line

  const startNewGame = useCallback(async (e: FormEvent) => {
    e.preventDefault()
    try {
      const createdGame = await fetch(`${BASE_URL}/games`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({ id: newGame })
      }).then(r => {
        if (r.status === 409) {
          if (games[newGame]) {
            setScreen(newGame)
          } else {
            alert('Game already exists, please refresh the page and select it.')
          }
          throw new Error('Game already exists')
        } else if (r.status === 503) {
          alert(`${newGame} may be offline, please confirm it is online and try again.`)
          throw new Error('Player offline')
        } else if (r.status === 400) {
          alert('Please enter a valid player ID')
          throw new Error('Invalid player ID')
        } else if (r.status > 399) {
          alert('There was an error creating the game. Please try again.')
          throw new Error('Error creating game')
        }

        return r.json()
      })

      const allGames = { ...games }
      allGames[createdGame.id] = createdGame
      set({ games: allGames })
      setScreen(newGame)
      setNewGame('')
    } catch (err) {
      console.error(err)
    }
  }, [games, newGame, setNewGame, set])

  const onDrop = useCallback((sourceSquare: string, targetSquare: string) => {
    if (!game || !isTurn(game, window.our.node)) return false

    const move = {
      from: sourceSquare,
      to: targetSquare,
      promotion: "q", // always promote to a queen for example simplicity
    }
    const gameCopy = { ...game };
    const result = gameCopy.game.move(move);

    if (result === null) {
      return false;
    }

    gameCopy.board = gameCopy.game.fen()
    const allGames = { ...games }
    allGames[game.id] = gameCopy
    set({ games: allGames })

    fetch(`${BASE_URL}/games`, {
      method: 'PUT',
      body: JSON.stringify({ id: game.id, move: sourceSquare + targetSquare })
    }).then(r => r.json())
    .then((updatedGame) => {
      const allGames = { ...games }
      allGames[game.id] = updatedGame
      set({ games: allGames })
    })
    .catch((err) => {
      console.error(err)
      alert('There was an error making your move. Please try again')
      // reset the board
      const allGames = { ...games }
      const gameCopy = { ...game }
      gameCopy.game.undo()
      allGames[game.id] = gameCopy
      set({ games: allGames })
    })

    return true
  }, [game, games, set])

  const resignGame = useCallback((e: MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    if (!game) return

    if (!window.confirm('Are you sure you want to resign this game?')) return

    fetch(`${BASE_URL}/games?id=${game.id}`, {
      method: 'DELETE',
    }).then(r => r.json())
    .then((updatedGame) => {
      const allGames = { ...games }
      allGames[game.id] = updatedGame
      set({ games: allGames })
    })
    .catch((err) => {
      console.error(err)
      alert('There was an error resigning the game. Please try again')
    })
  }, [game])

  const rematchGame = useCallback(async (e: MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    if (!game) return

    try {
      const createdGame = await fetch(`${BASE_URL}/games`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({ id: game.id })
      }).then(r => r.json())

      const allGames = { ...games }
      allGames[createdGame.id] = createdGame
      set({ games: allGames })
    } catch (err) {
      console.error(err)
      alert('You could not create the game. Please make sure your current game with this player (if any) has ended and try again.')
    }
  }, [game])

  return (
    <div className='flex flex-col justify-center items-center'>
      <div className='flex flex-col justify-center' style={{ maxHeight: '100vh', maxWidth: '800px', width: '100%', position: 'relative' }}>
        <a href="/" className='absolute top-6 left-0 m-4' style={{ fontSize: 24, color: 'white' }} onClick={e => { e.preventDefault(); window.history.back() }}>
          &#x25c0; Back
        </a>
        <h1 className='m-4'>Chess by Uqbar</h1>
        <div className='flex flex-row justify-center items-center h-screen border rounded'>
          {Object.keys(games).length > 0 && <div className='flex flex-col border-r' style={{ width: '25%', height: '100%' }}>
            <h3 className='m-2'>Games</h3>
            <button className='bg-green-600 hover:bg-green-800 text-white font-bold py-2 px-4 m-2 rounded' onClick={() => setScreen('new')}>New</button>
            <div className='flex flex-col overflow-scroll'>
              {Object.values(games).map(game => (
                <div key={game?.id} onClick={() => setScreen(game?.id)}
                  className={`game-entry m-2 ${screen !== game?.id && isTurn(game, window.our.node) ? 'is-turn' : ''} ${screen === game?.id ? 'selected' : ''} ${game?.ended ? 'ended' : ''}`}
                >
                  {game?.id}
                </div>
              ))}
            </div>
          </div>}
          <div className='flex flex-col justify-center items-center' style={{ width: '75%' }}>
            {screen === 'new' || !game ? (
              <>
                <h2 className='mb-2'>Start New Game</h2>
                <h4 className='mb-2'>(game creator will be white)</h4>
                <form onSubmit={startNewGame} className='flex flex-col justify-center mb-40' style={{ maxWidth: 400 }}>
                  <label className='mb-2' style={{ alignSelf: 'flex-start', fontWeight: '600' }}>Player ID</label>
                  <input className='border rounded p-2 mb-2' style={{ color: 'black' }} type='text' placeholder='Player ID' value={newGame} onChange={e => setNewGame(e.target.value)} />
                  <button className='bg-green-600 hover:bg-green-800 text-white font-bold py-2 px-4 rounded' type="submit">Start Game</button>
                </form>
              </>
            ) : (
              <>
                <div className='flex flex-row justify-between items-center w-full px-4 pb-2'>
                  <h3>{screen}</h3>
                  <h4>{game?.ended ? 'Game Ended' : `Turn: ${currentTurn}`}</h4>
                  {game?.ended ? (
                    <button className='bg-green-600 hover:bg-green-800 text-white font-bold py-1 px-4 rounded' onClick={rematchGame}>Rematch</button>
                  ) : (
                    <button className='bg-green-600 hover:bg-green-800 text-white font-bold py-1 px-4 rounded' onClick={resignGame}>Resign</button>
                  )}
                </div>
                <Chessboard position={game?.game.fen()} onPieceDrop={onDrop} boardOrientation={game?.white === window.our.node ? 'white' : 'black'} />
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

export default App
