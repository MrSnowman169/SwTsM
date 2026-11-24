<div align="center">

# ⚡ SwTsM

**A fast, beautiful and themeable Task Manager for Windows — written in Rust**

**Быстрый, красивый диспетчер задач для Windows с темами — на Rust**

**Швидкий, гарний диспетчер завдань для Windows з темами — на Rust**

![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6?logo=windows&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-egui-000000?logo=rust&logoColor=white)
![Languages](https://img.shields.io/badge/UI-EN%20%7C%20RU%20%7C%20UK-58A6FF)
![Made by](https://img.shields.io/badge/Made%20by-MrSnowman-111111)

[English](#-english) · [Русский](#-русский) · [Українська](#-українська)

</div>

---

## 🇬🇧 English

### ✨ Features

| | |
|---|---|
| ⌨️ **Ctrl+Shift+Esc** | Opens SwTsM instead of the standard Task Manager (optional, one click) |
| 📋 **Processes** | Real app icons, grouping by name, sorting, search (`Ctrl+F`), heat-map for CPU / memory / disk |
| 🖱️ **Right-click menu** | End task, end process tree, suspend / resume, set priority, open file location, search online, copy |
| 📈 **Performance** | Live 60-second graphs for CPU (incl. per core), memory, disk I/O and every network adapter, volume usage |
| 👤 **Users** | Processes grouped by owner |
| 🔍 **Details** | Full table: PID, parent, user, CPU, memory, virtual memory, read/write, uptime, path, command line |
| 🚀 **Startup apps** | Enable / disable autostart entries exactly like Windows does |
| 🔧 **Services** | Start, stop and restart Windows services |
| 🎨 **Themes** | **Liquid Glass** (real blur/acrylic), **Default** (your own RGB palette), **Neon**, **Nord**, **Like-Telegram**, **Like-Discord** |
| 🔤 **Text** | Text color (Auto / Black / Grey / White) and text size (90–150 %) |
| 🌍 **Languages** | English, Русский, Українська |
| 🛡️ **Safety** | Confirmation before ending critical system processes or a whole process tree |

### 📦 Installation

1. Download **`SwTsM-Setup.exe`** from the [Releases](../../releases) page.
2. Run it — setup asks for administrator rights (needed for *Program Files* and Ctrl+Shift+Esc; you can decline and install for the current user only).
3. Choose the **language** → choose the **install folder** and options → **Install**.

Silent install: `SwTsM-Setup.exe --silent [--dir=PATH] [--lang=En|Ru|Uk] [--no-shortcuts] [--hotkey]`

### 🗑️ Uninstall

*Settings → Uninstall SwTsM* (a 3-second confirmation protects from accidental clicks), or Windows *Settings → Apps*.
Everything is removed: files, shortcuts, settings, and Ctrl+Shift+Esc goes back to the standard Task Manager.

### ⌨️ Shortcuts

| Key | Action |
|---|---|
| `Ctrl+F` | Search |
| `Delete` | End selected task |
| `F5` | Refresh now |
| Right click | Context menu |

### 🔧 How Ctrl+Shift+Esc works

The same way as Microsoft's Process Explorer: SwTsM is registered as the `Debugger` for `taskmgr.exe` under
`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options`.
Ctrl+Shift+Esc, the taskbar menu and the Ctrl+Alt+Del screen then open SwTsM. Turning it off removes the value.

### 🛠️ Build from source

Requirements: Windows 10/11, [Rust](https://rustup.rs) (stable, MSVC or GNU toolchain).

```powershell
git clone https://github.com/MrSnowman169/SwTsM.git
cd SwTsM
.\build.ps1          # → dist\SwTsM.exe and dist\SwTsM-Setup.exe
```

The installer embeds the already built `SwTsM.exe`, so the app must be built first (`build.ps1` does it in the right order).

---

## 🇷🇺 Русский

### ✨ Возможности

| | |
|---|---|
| ⌨️ **Ctrl+Shift+Esc** | Открывает SwTsM вместо стандартного диспетчера (включается одной кнопкой) |
| 📋 **Процессы** | Настоящие иконки программ, группировка по имени, сортировка, поиск (`Ctrl+F`), подсветка нагрузки ЦП / памяти / диска |
| 🖱️ **Меню по ПКМ** | Снять задачу, завершить дерево процессов, приостановить / возобновить, приоритет, расположение файла, поиск в интернете, копирование |
| 📈 **Производительность** | Живые графики за 60 секунд: ЦП (в том числе по ядрам), память, диск и каждый сетевой адаптер, заполненность томов |
| 👤 **Пользователи** | Процессы, сгруппированные по владельцу |
| 🔍 **Подробности** | Полная таблица: PID, родитель, пользователь, ЦП, память, вирт. память, чтение/запись, время работы, путь, командная строка |
| 🚀 **Автозагрузка** | Включение и отключение программ так же, как это делает Windows |
| 🔧 **Службы** | Запуск, остановка и перезапуск служб Windows |
| 🎨 **Темы** | **Liquid Glass** (настоящее размытие/акрил), **Default** (своя RGB-палитра), **Neon**, **Nord**, **Like-Telegram**, **Like-Discord** |
| 🔤 **Текст** | Цвет текста (Авто / Чёрный / Серый / Белый) и размер (90–150 %) |
| 🌍 **Языки** | English, Русский, Українська |
| 🛡️ **Безопасность** | Подтверждение перед завершением критических системных процессов и дерева процессов |

### 📦 Установка

1. Скачайте **`SwTsM-Setup.exe`** на странице [Releases](../../releases).
2. Запустите — установщик запросит права администратора (нужны для *Program Files* и Ctrl+Shift+Esc; можно отказаться и установить только для текущего пользователя).
3. Выберите **язык** → **папку установки** и параметры → **Установить**.

Тихая установка: `SwTsM-Setup.exe --silent [--dir=ПУТЬ] [--lang=En|Ru|Uk] [--no-shortcuts] [--hotkey]`

### 🗑️ Удаление

*Параметры → Удалить SwTsM* (3-секундный отсчёт защищает от случайного нажатия) или *Параметры Windows → Приложения*.
Удаляется всё: файлы, ярлыки, настройки, а Ctrl+Shift+Esc снова открывает стандартный диспетчер.

### ⌨️ Сочетания клавиш

| Клавиша | Действие |
|---|---|
| `Ctrl+F` | Поиск |
| `Delete` | Снять выбранную задачу |
| `F5` | Обновить сейчас |
| ПКМ | Контекстное меню |

### 🔧 Как работает Ctrl+Shift+Esc

Так же, как у Process Explorer от Microsoft: SwTsM прописывается как `Debugger` для `taskmgr.exe` в
`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options`.
После этого Ctrl+Shift+Esc, меню панели задач и экран Ctrl+Alt+Del открывают SwTsM. Выключение удаляет это значение.

### 🛠️ Сборка из исходников

Нужны: Windows 10/11, [Rust](https://rustup.rs) (stable, тулчейн MSVC или GNU).

```powershell
git clone https://github.com/MrSnowman169/SwTsM.git
cd SwTsM
.\build.ps1          # → dist\SwTsM.exe и dist\SwTsM-Setup.exe
```

Установщик вшивает в себя уже собранный `SwTsM.exe`, поэтому приложение собирается первым (`build.ps1` делает всё в нужном порядке).

---

## 🇺🇦 Українська

### ✨ Можливості

| | |
|---|---|
| ⌨️ **Ctrl+Shift+Esc** | Відкриває SwTsM замість стандартного диспетчера (вмикається однією кнопкою) |
| 📋 **Процеси** | Справжні іконки програм, групування за ім'ям, сортування, пошук (`Ctrl+F`), підсвічування навантаження ЦП / пам'яті / диска |
| 🖱️ **Меню по ПКМ** | Зняти завдання, завершити дерево процесів, призупинити / відновити, пріоритет, розташування файлу, пошук в Інтернеті, копіювання |
| 📈 **Продуктивність** | Живі графіки за 60 секунд: ЦП (зокрема по ядрах), пам'ять, диск і кожен мережевий адаптер, заповненість томів |
| 👤 **Користувачі** | Процеси, згруповані за власником |
| 🔍 **Подробиці** | Повна таблиця: PID, батьківський процес, користувач, ЦП, пам'ять, вірт. пам'ять, читання/запис, час роботи, шлях, командний рядок |
| 🚀 **Автозавантаження** | Увімкнення та вимкнення програм так само, як це робить Windows |
| 🔧 **Служби** | Запуск, зупинка та перезапуск служб Windows |
| 🎨 **Теми** | **Liquid Glass** (справжнє розмиття/акрил), **Default** (власна RGB-палітра), **Neon**, **Nord**, **Like-Telegram**, **Like-Discord** |
| 🔤 **Текст** | Колір тексту (Авто / Чорний / Сірий / Білий) і розмір (90–150 %) |
| 🌍 **Мови** | English, Русский, Українська |
| 🛡️ **Безпека** | Підтвердження перед завершенням критичних системних процесів і дерева процесів |

### 📦 Встановлення

1. Завантажте **`SwTsM-Setup.exe`** на сторінці [Releases](../../releases).
2. Запустіть — інсталятор попросить права адміністратора (потрібні для *Program Files* і Ctrl+Shift+Esc; можна відмовитися та встановити лише для поточного користувача).
3. Оберіть **мову** → **теку встановлення** та параметри → **Встановити**.

Тихе встановлення: `SwTsM-Setup.exe --silent [--dir=ШЛЯХ] [--lang=En|Ru|Uk] [--no-shortcuts] [--hotkey]`

### 🗑️ Видалення

*Параметри → Видалити SwTsM* (3-секундний відлік захищає від випадкового натискання) або *Параметри Windows → Програми*.
Видаляється все: файли, ярлики, налаштування, а Ctrl+Shift+Esc знову відкриває стандартний диспетчер.

### ⌨️ Сполучення клавіш

| Клавіша | Дія |
|---|---|
| `Ctrl+F` | Пошук |
| `Delete` | Зняти вибране завдання |
| `F5` | Оновити зараз |
| ПКМ | Контекстне меню |

### 🔧 Як працює Ctrl+Shift+Esc

Так само, як у Process Explorer від Microsoft: SwTsM прописується як `Debugger` для `taskmgr.exe` у
`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options`.
Після цього Ctrl+Shift+Esc, меню панелі завдань і екран Ctrl+Alt+Del відкривають SwTsM. Вимкнення видаляє це значення.

### 🛠️ Збирання з вихідного коду

Потрібно: Windows 10/11, [Rust](https://rustup.rs) (stable, тулчейн MSVC або GNU).

```powershell
git clone https://github.com/MrSnowman169/SwTsM.git
cd SwTsM
.\build.ps1          # → dist\SwTsM.exe і dist\SwTsM-Setup.exe
```

Інсталятор вбудовує вже зібраний `SwTsM.exe`, тому програма збирається першою (`build.ps1` робить усе в правильному порядку).

---

<div align="center">

**Made by MrSnowman** ❄️

</div>
