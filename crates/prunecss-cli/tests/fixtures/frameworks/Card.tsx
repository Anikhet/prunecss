import styles from "./Card.module.css";

export const Card = ({ big }: { big: boolean }) => (
  <div className={`card ${big ? "card-lg" : ""} shadow md:flex w-1/2 grid-cols-[repeat(auto-fill,minmax(0,1fr))]`}>
    <span className={styles["module-button"]} />
  </div>
);

export function openModal() {
  document.querySelector(".modal")?.classList.add("open");
}
