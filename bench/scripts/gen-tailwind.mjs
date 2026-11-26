// Generates the Tailwind corpus: realistic HTML pages that use Tailwind v4
// utilities (responsive/state variants, fractions, arbitrary values), and a
// Tailwind input stylesheet whose broad `@source inline()` safelist makes the
// built CSS much larger than what the pages use, as a "ship everything"
// build would be. The CSS itself is built by fetch-corpus.mjs with the pinned
// @tailwindcss/cli.
import { join } from 'node:path';
import { rng, writeFile } from '../lib/util.mjs';

const COLORS = ['slate', 'gray', 'red', 'orange', 'amber', 'green', 'emerald', 'teal', 'sky', 'blue', 'indigo', 'violet', 'pink', 'rose'];
const SHADES = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];

export function tailwindInputCss() {
  // Brace expansion is Tailwind v4's safelist syntax. Each line multiplies
  // out to thousands of utilities, most of which no page uses.
  const variants = '{,sm:,md:,lg:,xl:,2xl:,hover:,focus:,dark:,group-hover:}';
  const spacing = '{0,0.5,1,1.5,2,2.5,3,4,5,6,7,8,9,10,11,12,14,16,20,24,28,32,36,40,44,48,52,56,60,64,72,80,96,px}';
  const colors = `{${COLORS.join(',')}}-{50,{100..900..100},950}`;
  return `@import "tailwindcss";
@source "./pages";
@source inline("${variants}{p,px,py,pt,pr,pb,pl,m,mx,my,mt,mr,mb,ml,gap,gap-x,gap-y,space-x,space-y}-${spacing}");
@source inline("${variants}{w,h,min-w,min-h,max-h,size}-${spacing}");
@source inline("${variants}{w,h}-{1/2,1/3,2/3,1/4,2/4,3/4,1/5,2/5,3/5,4/5,full,screen,auto,fit,min,max}");
@source inline("${variants}{bg,text,border,ring,divide,outline,from,via,to,fill,stroke,shadow,accent,decoration,placeholder}-${colors}");
@source inline("${variants}{flex,inline-flex,grid,inline-grid,block,inline-block,hidden,table,contents,flow-root}");
@source inline("${variants}{grid-cols,col-span,row-span,grid-rows,order,columns}-{1,2,3,4,5,6,7,8,9,10,11,12}");
@source inline("${variants}{text,font,leading,tracking,rounded,shadow,blur,opacity}-{xs,sm,base,lg,xl,2xl,3xl,4xl,5xl,6xl,7xl,8xl,9xl,thin,light,normal,medium,semibold,bold,extrabold,black,none,tight,snug,relaxed,loose,wide,wider,widest,full,inner,md,0,5,10,25,50,75,90,95,100}");
@source inline("${variants}{items,justify,content,self,place-items,place-content}-{start,end,center,between,around,evenly,stretch,baseline}");
@source inline("${variants}{translate-x,translate-y,rotate,scale,skew-x,skew-y,z,inset,top,right,bottom,left}-{0,1,2,4,6,8,10,12,45,90,95,100,105,110,125,150,auto,full,1/2}");
`;
}

const pad = (r) => `p-${r.pick([2, 3, 4, 6, 8])}`;
const color = (r, prefix) => `${prefix}-${r.pick(COLORS)}-${r.pick(SHADES)}`;

function navbar(r) {
  const links = Array.from({ length: 3 + r.int(4) }, (_, i) =>
    `      <a href="#" class="${r.pick(['text-sm', 'text-base'])} font-medium ${color(r, 'text')} hover:${color(r, 'text')} focus-visible:outline-2 ${r.chance(0.3) ? 'underline-offset-4 hover:underline' : ''}">Link ${i}</a>`,
  ).join('\n');
  return `  <header class="sticky top-0 z-50 w-full border-b ${color(r, 'border')} ${color(r, 'bg')}/80 backdrop-blur supports-[backdrop-filter]:bg-white/60 dark:bg-gray-900">
    <nav class="mx-auto flex max-w-7xl items-center justify-between ${pad(r)} lg:px-8" aria-label="Global">
      <a href="#" class="-m-1.5 p-1.5"><img class="h-8 w-auto" src="logo.svg" alt=""></a>
      <button type="button" class="-m-2.5 inline-flex items-center justify-center rounded-md p-2.5 text-gray-700 md:hidden">Menu</button>
      <div class="hidden md:flex md:gap-x-${r.pick([6, 8, 12])}">
${links}
      </div>
    </nav>
  </header>`;
}

function hero(r) {
  return `  <section class="relative isolate overflow-hidden ${color(r, 'bg')} py-24 sm:py-32">
    <div class="mx-auto max-w-2xl text-center lg:max-w-4xl">
      <h1 class="text-4xl font-bold tracking-tight ${color(r, 'text')} sm:text-6xl">Ship faster</h1>
      <p class="mt-6 text-lg leading-8 text-gray-600 dark:text-gray-300">Lorem ipsum dolor sit amet.</p>
      <div class="mt-10 flex items-center justify-center gap-x-6">
        <a href="#" class="rounded-md ${color(r, 'bg')} px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:${color(r, 'bg')} focus-visible:outline focus-visible:outline-offset-2 active:scale-95 transition-colors duration-150">Get started</a>
        <a href="#" class="text-sm font-semibold leading-6 text-gray-900 [&>span]:ml-1">Learn more <span aria-hidden="true">&rarr;</span></a>
      </div>
    </div>
    <div class="absolute inset-x-0 -top-40 -z-10 transform-gpu blur-3xl sm:-top-80 w-[36rem] h-[10px] bg-[#1da1f2] rotate-[30deg]"></div>
  </section>`;
}

function cards(r) {
  const n = 3 + r.int(6);
  const cols = r.pick(['md:grid-cols-2 lg:grid-cols-3', 'sm:grid-cols-2 xl:grid-cols-4', 'grid-cols-[200px_1fr]', 'md:grid-cols-3']);
  const items = Array.from({ length: n }, (_, i) => `      <article class="group relative flex flex-col overflow-hidden rounded-${r.pick(['lg', 'xl', '2xl'])} border ${color(r, 'border')} bg-white shadow-sm hover:shadow-lg dark:bg-gray-800 ${r.chance(0.4) ? 'w-1/2 md:w-1/3' : 'w-full'}">
        <img class="aspect-video w-full object-cover group-hover:opacity-75" src="img${i}.jpg" alt="">
        <div class="flex flex-1 flex-col space-y-${r.pick([2, 3, 4])} ${pad(r)}">
          <h3 class="text-${r.pick(['base', 'lg', 'xl'])} font-semibold ${color(r, 'text')} group-hover:${color(r, 'text')}">Card ${i}</h3>
          <p class="line-clamp-3 text-sm text-gray-500 dark:text-gray-400">Card body text.</p>
          <span class="inline-flex w-fit items-center rounded-full ${color(r, 'bg')} px-2 py-1 text-xs font-medium ${color(r, 'text')} ring-1 ring-inset ${color(r, 'ring')}">Tag</span>
        </div>
      </article>`).join('\n');
  return `  <section class="mx-auto max-w-7xl px-4 py-16 sm:px-6 lg:px-8">
    <div class="grid grid-cols-1 gap-${r.pick([4, 6, 8])} ${cols}">
${items}
    </div>
  </section>`;
}

function form(r) {
  const fields = Array.from({ length: 2 + r.int(5) }, (_, i) => `      <div class="sm:col-span-${r.pick([2, 3, 4, 6])}">
        <label for="f${i}" class="block text-sm font-medium leading-6 text-gray-900">Field ${i}</label>
        <input id="f${i}" type="${r.pick(['text', 'email', 'password'])}" class="peer mt-2 block w-full rounded-md border-0 py-1.5 text-gray-900 shadow-sm ring-1 ring-inset ring-gray-300 placeholder:text-gray-400 focus:ring-2 focus:ring-inset ${r.chance(0.5) ? 'focus:ring-indigo-600' : 'focus:ring-emerald-500'} invalid:border-pink-500 sm:text-sm sm:leading-6 disabled:cursor-not-allowed disabled:opacity-50">
        <p class="mt-2 hidden text-sm text-red-600 peer-invalid:block">Invalid value.</p>
      </div>`).join('\n');
  return `  <form class="mx-auto max-w-2xl space-y-8 divide-y divide-gray-200 ${pad(r)}">
    <div class="grid grid-cols-1 gap-x-6 gap-y-8 sm:grid-cols-6">
${fields}
      <label class="inline-flex items-center gap-2"><input type="checkbox" class="peer size-4 rounded border-gray-300 accent-indigo-600"><span class="text-sm peer-checked:font-semibold">Remember me</span></label>
    </div>
    <button type="submit" class="w-full rounded-md bg-indigo-600 px-3 py-2 text-sm font-semibold text-white hover:bg-indigo-500 sm:w-auto data-[loading=true]:opacity-50 aria-disabled:cursor-not-allowed">Save</button>
  </form>`;
}

function table(r) {
  const rows = Array.from({ length: 3 + r.int(8) }, (_, i) => `        <tr class="even:bg-gray-50 hover:bg-gray-100 dark:even:bg-gray-800"><td class="whitespace-nowrap py-4 pl-4 pr-3 text-sm font-medium text-gray-900 sm:pl-0">Row ${i}</td><td class="px-3 py-4 text-sm text-gray-500 tabular-nums">${r.int(1000)}</td><td class="relative py-4 pl-3 pr-4 text-right text-sm sm:pr-0"><a href="#" class="${color(r, 'text')} hover:underline">Edit</a></td></tr>`).join('\n');
  return `  <div class="-mx-4 mt-8 overflow-x-auto sm:-mx-0">
    <table class="min-w-full table-auto divide-y divide-gray-300">
      <thead class="bg-gray-50"><tr><th scope="col" class="py-3.5 pl-4 pr-3 text-left text-sm font-semibold text-gray-900 sm:pl-0">Name</th><th class="px-3 py-3.5 text-left text-sm font-semibold">Value</th><th class="relative py-3.5 pl-3 pr-4 sm:pr-0"><span class="sr-only">Edit</span></th></tr></thead>
      <tbody class="divide-y divide-gray-200 bg-white">
${rows}
      </tbody>
    </table>
  </div>`;
}

function pricing(r) {
  const tiers = Array.from({ length: 3 }, (_, i) => `      <div class="flex flex-col justify-between rounded-3xl ${i === 1 ? 'bg-gray-900 ring-gray-900 text-white scale-105' : 'bg-white ring-gray-200'} p-8 ring-1 xl:p-10 max-w-[24rem] min-h-[calc(100vh-4rem)] lg:min-h-0">
        <h3 class="text-lg font-semibold leading-8">Tier ${i}</h3>
        <p class="mt-6 flex items-baseline gap-x-1"><span class="text-4xl font-bold tracking-tight">$${(i + 1) * 15}</span><span class="text-sm font-semibold leading-6 text-gray-600">/month</span></p>
        <ul role="list" class="mt-8 space-y-3 text-sm leading-6 text-gray-600 xl:mt-10 *:flex *:gap-x-3">
          <li>Feature A</li><li>Feature B</li>
        </ul>
        <a href="#" class="mt-8 block rounded-md px-3 py-2 text-center text-sm font-semibold ${color(r, 'bg')} hover:opacity-90 first-letter:uppercase">Buy plan</a>
      </div>`).join('\n');
  return `  <section class="isolate mx-auto mt-16 grid max-w-md grid-cols-1 gap-y-8 sm:mt-20 lg:mx-0 lg:max-w-none lg:grid-cols-3">
${tiers}
  </section>`;
}

function footer(r) {
  return `  <footer class="${color(r, 'bg')} mt-auto" aria-labelledby="footer-heading">
    <div class="mx-auto max-w-7xl px-6 pb-8 pt-16 sm:pt-24 lg:px-8 lg:pt-32">
      <div class="xl:grid xl:grid-cols-3 xl:gap-8">
        <p class="text-sm leading-6 text-gray-300 md:order-1 md:mt-0">&copy; 2026 Example, Inc.</p>
        <div class="mt-16 grid grid-cols-2 gap-8 xl:col-span-2 xl:mt-0 print:hidden motion-safe:animate-pulse">
          <a href="#" class="text-sm leading-6 text-gray-300 hover:text-white">About</a>
        </div>
      </div>
    </div>
  </footer>`;
}

const SECTIONS = [hero, cards, form, table, pricing, cards];

/** Writes `count` pages to `dir/pages/` and the Tailwind input CSS to `dir/input.css`. */
export function generateTailwind(dir, count = 40, seed = 0x7a11) {
  const r = rng(seed);
  for (let p = 0; p < count; p++) {
    const sections = Array.from({ length: 2 + r.int(4) }, () => r.pick(SECTIONS)(r));
    const html = `<!doctype html>
<html lang="en" class="h-full scroll-smooth antialiased">
<head><meta charset="utf-8"><title>Page ${p}</title><link rel="stylesheet" href="../tailwind.css"></head>
<body class="flex min-h-full flex-col bg-white font-sans text-gray-900 dark:bg-gray-950 dark:text-gray-100">
${navbar(r)}
  <main class="flex-1">
${sections.join('\n')}
  </main>
${footer(r)}
</body>
</html>
`;
    writeFile(join(dir, 'pages', `page-${String(p).padStart(3, '0')}.html`), html);
  }
  writeFile(join(dir, 'input.css'), tailwindInputCss());
}
