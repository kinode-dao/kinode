import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"
import { useEffect, useState } from "react"
import { DragDropContext, Draggable, DropResult, Droppable } from '@hello-pangea/dnd'

const AppsDock: React.FC = () => {
  const { apps } = useHomepageStore()
  const [dockedApps, setDockedApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    const orderedApps = apps
      .filter(a => a.favorite)
      .sort((a, b) => (a.order ?? Number.MAX_SAFE_INTEGER) - (b.order ?? Number.MAX_SAFE_INTEGER))

    setDockedApps(orderedApps)
  }, [apps])

  // a little function to help us with reordering the result
  const reorder = (list: HomepageApp[], startIndex: number, endIndex: number) => {
    const result = Array.from(list);
    const [removed] = result.splice(startIndex, 1);
    result.splice(endIndex, 0, removed);

    return removed;
  };

  const onDragEnd = (result: DropResult) => {
    // dropped outside the list
    if (!result.destination) {
      return;
    }

    const app = reorder(
      dockedApps,
      result.source.index,
      result.destination.index
    );

    fetch('/favorite', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      credentials: 'include',
      body: JSON.stringify([app.id, app.order, app.favorite])
    })
      .catch(e => console.error(e));
  }

  return <DragDropContext onDragEnd={onDragEnd}>
    <Droppable droppableId="droppable" direction="horizontal">
      {(provided, _snapshot) => (
        <div
          ref={provided.innerRef}
          {...provided.droppableProps}
        >
          {dockedApps.map(app => <Draggable
            key={app.id}
            draggableId={app.id}
            index={dockedApps.indexOf(app)}
          >
            {(provided, _snapshot) => (
              <div
                ref={provided.innerRef}
                {...provided.draggableProps}
                {...provided.dragHandleProps}
                className="docked-app"
              >
                <AppDisplay app={app} />
              </div>
            )}
          </Draggable>)}
        </div>
      )}
    </Droppable>
  </DragDropContext>
}

export default AppsDock