/** One type hierarchy for dialogs, using the application's shared font scale. */
export const modalTypography = [
  'font-app text-control font-normal text-slate-700 workspace-dark:text-slate-200',
  '[&_h2]:text-subheading! [&_h2]:font-medium! [&_h2]:tracking-tight',
  '[&_h2]:text-slate-800! workspace-dark:[&_h2]:text-slate-100!',
  '[&_h3]:text-control! [&_h3]:font-medium!',
  '[&_label]:text-control! [&_label]:font-normal!',
  '[&_label]:text-slate-500! workspace-dark:[&_label]:text-slate-400!',
  '[&_p:not(.eyebrow)]:text-control! [&_p]:font-normal! [&_p]:leading-relaxed',
  '[&_small]:text-detail! [&_small]:font-normal!',
  '[&_strong]:font-medium!',
].join(' ');
