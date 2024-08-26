import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import usePersistentStore from "../store/persistentStore"
import AppDisplay from "./AppDisplay"
import { useEffect, useState } from "react"
import { DragDropContext, Draggable, DropResult, Droppable } from '@hello-pangea/dnd'

const AppsDock: React.FC = () => {
  const { apps } = useHomepageStore()
  const { appOrder, setAppOrder } = usePersistentStore()
  const [dockedApps, setDockedApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    // Sort apps based on persisted order
    const orderedApps = apps.filter(app => app.favorite).sort((a, b) => {
      return appOrder.indexOf(a.id) - appOrder.indexOf(b.id);
    });
    setDockedApps(orderedApps);

    // Sync the order with the backend
    fetch('/order', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      credentials: 'include',
      body: JSON.stringify(orderedApps.map(app => [app.id, appOrder.indexOf(app.id)]))
    });
  }, [apps, appOrder])

  const onDragEnd = (result: DropResult) => {
    if (!result.destination) {
      return;
    }

    const reorderedApps = Array.from(dockedApps);
    const [reorderedItem] = reorderedApps.splice(result.source.index, 1);
    reorderedApps.splice(result.destination.index, 0, reorderedItem);

    const newAppOrder = reorderedApps.map(app => app.id);
    setAppOrder(newAppOrder);
    setDockedApps(reorderedApps);

    fetch('/order', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      credentials: 'include',
      body: JSON.stringify(reorderedApps.map((app, index) => [app.id, index]))
    });
  }

  return <DragDropContext onDragEnd={onDragEnd}>
    <Droppable droppableId="droppable" direction="horizontal">
      {(provided, _snapshot) => (
        <div
          ref={provided.innerRef}
          {...provided.droppableProps}
        >
          {dockedApps.map((app, index) => <Draggable
            key={app.id}
            draggableId={app.id}
            index={index}
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