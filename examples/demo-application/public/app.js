// Small progressive enhancements. The app works without JavaScript.
document.addEventListener('DOMContentLoaded', () => {
  // Disable submit buttons on submit to avoid double orders.
  for (const form of document.querySelectorAll('form[action="/checkout"]')) {
    form.addEventListener('submit', () => {
      const button = form.querySelector('button[type="submit"]');
      if (button) {
        button.disabled = true;
        button.textContent = 'Placing order…';
      }
    });
  }
  // Focus the first error message for screen-reader users.
  const alert = document.querySelector('[role="alert"]');
  if (alert) {
    alert.setAttribute('tabindex', '-1');
    alert.focus();
  }
});
