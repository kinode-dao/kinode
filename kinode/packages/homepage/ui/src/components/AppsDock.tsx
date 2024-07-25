import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"
import { useEffect, useState } from "react"
import { DragDropContext, Draggable, DropResult, Droppable } from 'react-beautiful-dnd'

const AppsDock: React.FC = () => {
  const { apps } = useHomepageStore()
  const [dockedApps, setDockedApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    let final: HomepageApp[] = []
    const orderedApps = dockedApps.filter(a => a.order !== undefined && a.order !== null)
    const unorderedApps = dockedApps.filter(a => a.order === undefined || a.order === null)

    for (let i = 0; i < orderedApps.length; i++) {
      final[orderedApps[i].order!] = orderedApps[i]
    }

    final = final.filter(a => a)
    unorderedApps.forEach(a => final.push(a))
    setDockedApps(final)
  }, [apps])

  // a little function to help us with reordering the result
  const reorder = (list: HomepageApp[], startIndex: number, endIndex: number) => {
    const result = Array.from(list);
    const [removed] = result.splice(startIndex, 1);
    result.splice(endIndex, 0, removed);

    return result;
  };

  const onDragEnd = (result: DropResult) => {
    // dropped outside the list
    if (!result.destination) {
      return;
    }

    const items = reorder(
      dockedApps,
      result.source.index,
      result.destination.index
    );

    const packageNames = items.map(app => app.package_name);

    fetch('/order', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json'
      },
      credentials: 'include',
      body: JSON.stringify(packageNames)
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
          {/*dockedApps.length === 0
            ? <AppDisplay app={apps.find(app => app.package_name === 'app_store')!} />
            : */ dockedApps.map(app => <Draggable
            key={app.package_name}
            draggableId={app.package_name}
            index={dockedApps.indexOf(app)}
          >
            {(provided, _snapshot) => (
              <div
                ref={provided.innerRef}
                {...provided.draggableProps}
                {...provided.dragHandleProps}
              >
                <AppDisplay app={app} />
              </div>
            )}
          </Draggable>)}
          {provided.placeholder}
          {dockedApps.length === 0 && <div>Favorite an app to pin it to your dock.</div>}
        </div>
      )}
    </Droppable>
  </DragDropContext>
}

export default AppsDock