import clsx from 'clsx';

export function App({ on }) {
  return (
    <div>
      <button className={clsx('btn', on && 'clsx-on')}>Toggle</button>
      <section className={`card tpl-literal`}>Card</section>
    </div>
  );
}
