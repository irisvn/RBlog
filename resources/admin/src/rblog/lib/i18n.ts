// RBlog's React pages in Vietnamese (docs/ui-guidelines.md §9): `props.t` reads these catalogs.
// Every string a page shows has its line here, in the same change.
declare global {
  interface Window {
    rustcms?: { registerTranslations?: (locale: string, catalog: Record<string, string>) => void };
  }
}

window.rustcms?.registerTranslations?.("vi", {
  // Shared (lib/ui.tsx)
  Cancel: "Huỷ",
  Close: "Đóng",
  Previous: "Trước",
  Next: "Sau",
  "{from}–{to} of {total}": "{from}–{to} / {total}",
  Actions: "Thao tác",
  "Something went wrong. Try again.": "Đã có lỗi. Hãy thử lại.",
  "The answer was lost: the change may have been made. Check the list.":
    "Không nhận được phản hồi: thay đổi có thể đã được thực hiện. Hãy kiểm tra lại danh sách.",

  // Status words (lib/status.ts)
  Active: "Đang dùng",
  Expired: "Đã hết hạn",
  Published: "Đã đăng",
  "Pending review": "Chờ duyệt",
  Scheduled: "Đã lên lịch",
  Draft: "Nháp",

  // Menu item link types (menu_item_resource.rs, the rblog/option-label cell in content.tsx)
  URL: "URL",
  Post: "Bài viết",
  Page: "Trang",
  Category: "Danh mục",
  Tag: "Thẻ",

  // Preview of a post or page (content.tsx)
  "See it on the public site as readers will, before it goes live. A link works for one hour.":
    "Xem trên trang công khai đúng như người đọc sẽ thấy, trước khi đăng. Mỗi link dùng được một giờ.",
  "Set the public site URL in Settings → General to preview.":
    "Hãy đặt URL trang công khai trong Cài đặt → Chung để xem trước.",
  "Get preview link": "Lấy link xem trước",
  "New preview link": "Lấy link mới",
  "Making link…": "Đang tạo link…",
  "Copy link": "Copy link",
  "Link copied": "Đã copy link",
  "Could not copy. Select the link and copy it.": "Không copy được. Hãy chọn link rồi copy.",
  "Works until {time}": "Dùng được đến {time}",
  "This record can no longer be previewed.": "Không còn xem trước được mục này.",

  // Content API (content-api.tsx)
  Integrations: "Tích hợp",
  "Content API": "Content API",
  "Keys that let your public site read published posts, pages, menus and settings.":
    "Khoá cho trang công khai đọc bài viết, trang, menu và cài đặt đã đăng.",
  "New key": "Tạo khoá",
  Keys: "Khoá",
  "Each key reads published content only. Revoke a key you no longer use.":
    "Mỗi khoá chỉ đọc được nội dung đã đăng. Thu hồi khoá không còn dùng.",
  "No keys yet": "Chưa có khoá nào",
  "Create a key for each site that reads your content.": "Tạo một khoá cho mỗi site đọc nội dung của bạn.",
  Name: "Tên",
  Status: "Trạng thái",
  Created: "Ngày tạo",
  "Last used": "Dùng lần cuối",
  Expires: "Hết hạn",
  Never: "Không",
  "Not used yet": "Chưa dùng",
  "Expires {date}": "Hết hạn lúc {date}",
  "Does not expire": "Không hết hạn",
  "Actions for {name}": "Thao tác cho {name}",
  "Revoke…": "Thu hồi…",
  "Revoke the key {name}?": "Thu hồi khoá {name}?",
  "Sites that use it can no longer read your content. This cannot be undone.":
    "Các site đang dùng khoá này sẽ không đọc được nội dung nữa. Không thể hoàn tác.",
  "Revoke key": "Thu hồi khoá",
  "Revoking…": "Đang thu hồi…",
  "Key revoked": "Đã thu hồi khoá",
  "Using a key": "Cách dùng khoá",
  "Your public site sends the key with every request to the Content API.":
    "Trang công khai gửi khoá kèm mỗi request tới Content API.",
  "Base URL": "URL gốc",
  Header: "Header",
  Example: "Ví dụ",
  "Keep the key on the server (SSR): never send it to browsers or put it in client code.":
    "Chỉ giữ khoá ở phía server (SSR): không bao giờ gửi xuống trình duyệt hay đưa vào code phía client.",
  "Content API documentation": "Tài liệu Content API",
  "No expiry": "Không hết hạn",
  "30 days": "30 ngày",
  "90 days": "90 ngày",
  "365 days": "365 ngày",
  "Where the key is used, for example Production site. At most 80 characters.":
    "Nơi dùng khoá, ví dụ Site production. Tối đa 80 ký tự.",
  "An expired key stops working. You can revoke a key at any time.":
    "Khoá hết hạn sẽ ngừng hoạt động. Bạn có thể thu hồi khoá bất cứ lúc nào.",
  "Create key": "Tạo khoá",
  "Creating…": "Đang tạo…",
  "Key created": "Đã tạo khoá",
  "Copy your new key": "Copy khoá mới",
  "Copy it now: it is not shown again.": "Hãy copy ngay: khoá sẽ không hiện lại.",
  "Content API key": "Khoá Content API",
  "Copy key": "Copy khoá",
  Copied: "Đã copy",
  "Key copied": "Đã copy khoá",
  "Could not copy. Select the key and copy it.": "Không copy được. Hãy chọn khoá rồi copy.",

  // Version status words (lib/status.ts)
  "Update available": "Có bản cập nhật",
  "Up to date": "Mới nhất",
  "No release yet": "Chưa có bản phát hành",
  "Not checked": "Chưa kiểm tra",
  "Not compared": "Không so sánh",

  // Version (version.tsx)
  System: "Hệ thống",
  Version: "Phiên bản",
  "What RBlog runs, and whether a newer release is out. RBlog never updates itself: you update the containers.":
    "RBlog đang chạy bản nào, và đã có bản mới chưa. RBlog không bao giờ tự cập nhật: bạn tự cập nhật container.",
  "Check now": "Kiểm tra ngay",
  "Checking…": "Đang kiểm tra…",
  Checked: "Đã kiểm tra",
  "RBlog {version} is out": "Đã có RBlog {version}",
  "The check failed": "Kiểm tra thất bại",
  "Checked less than a minute ago. Try again shortly.": "Vừa kiểm tra chưa đầy một phút. Hãy thử lại sau ít giây.",
  "GitHub did not answer in time.": "GitHub không trả lời kịp.",
  "GitHub could not be reached. Check the server's internet access and DNS.":
    "Không kết nối được tới GitHub. Hãy kiểm tra kết nối Internet và DNS của server.",
  "GitHub's rate limit was reached. The next check will try again.":
    "Đã chạm giới hạn truy vấn của GitHub. Lần kiểm tra sau sẽ thử lại.",
  "GitHub answered with an error (HTTP {status}).": "GitHub trả về lỗi (HTTP {status}).",
  "GitHub's answer was too large to read.": "Phản hồi của GitHub quá lớn để đọc.",
  "GitHub's answer could not be read.": "Không đọc được phản hồi của GitHub.",
  "GitHub's 20 newest releases are all drafts or prereleases: the newest stable release was not found.":
    "20 bản mới nhất trên GitHub đều là bản nháp hoặc bản thử nghiệm: không tìm thấy bản ổn định mới nhất.",
  "A newer version of RBlog migrated this database ({names}). This version runs on it as it is: update again, or restore the backup made before the update.":
    "Một phiên bản RBlog mới hơn đã migrate database này ({names}). Phiên bản này vẫn chạy trên database như hiện tại: hãy cập nhật lại, hoặc khôi phục bản sao lưu đã tạo trước khi cập nhật.",
  Running: "Đang chạy",
  RBlog: "RBlog",
  Build: "Bản build",
  Release: "Bản phát hành",
  "Development build": "Bản phát triển",
  "Built from source": "Build từ mã nguồn",
  Commit: "Commit",
  Unknown: "Không rõ",
  Image: "Image",
  "Not a release image": "Không phải image phát hành",
  "RustCMS Core": "RustCMS Core",
  MySQL: "MySQL",
  Started: "Khởi động",
  "Release check": "Kiểm tra bản mới",
  "RBlog asks GitHub for new releases every {hours} hours, and sends nothing about this server.":
    "Cứ {hours} giờ RBlog hỏi GitHub một lần xem có bản mới không, và không gửi thông tin gì về server này.",
  "RBlog {version} is available": "Có bản mới {version}",
  "This is the newest release": "Đã là bản mới nhất",
  "No release of RBlog is published yet.": "RBlog chưa có bản phát hành nào.",
  "Not checked yet. Check now, or wait for the next automatic check.":
    "Chưa kiểm tra. Hãy bấm Kiểm tra ngay, hoặc chờ lần kiểm tra tự động tiếp theo.",
  "The newest release is {version}.": "Bản phát hành mới nhất là {version}.",
  "No release is known yet.": "Chưa biết bản phát hành nào.",
  "This is a development build: it is not compared with releases. Install a release image to be told about updates.":
    "Đây là bản phát triển: không được so sánh với các bản phát hành. Hãy cài image phát hành để được báo khi có bản mới.",
  "This build was made from source: it is not compared with releases. Install a release image to be told about updates.":
    "Bản này được build từ mã nguồn: không được so sánh với các bản phát hành. Hãy cài image phát hành để được báo khi có bản mới.",
  "The last check failed": "Lần kiểm tra gần nhất thất bại",
  "What is shown is from {when}.": "Thông tin đang hiện là từ {when}.",
  "Last checked": "Kiểm tra lần cuối",
  "Newest release": "Bản phát hành mới nhất",
  "You can check again in {seconds} s.": "Có thể kiểm tra lại sau {seconds} giây.",
  "What's new in {version}": "Có gì mới trong {version}",
  "Published {date}.": "Phát hành lúc {date}.",
  "View on GitHub": "Xem trên GitHub",
  "This release has no notes.": "Bản này không có ghi chú phát hành.",
  "How to update": "Cách cập nhật",
  "In the directory that holds compose.yml, .env and rblog.env on the server (docs/deployment.md).":
    "Chạy trong thư mục chứa compose.yml, .env và rblog.env trên server (docs/deployment.md).",
  "Back up the database": "Sao lưu database",
  "Database backup": "Sao lưu database",
  "Keep the file until the new version runs well. With the local disk, back up the storage volume too.":
    "Giữ file này đến khi bản mới chạy ổn. Nếu dùng ổ đĩa local, hãy sao lưu cả volume storage.",
  "Choose the version": "Chọn phiên bản",
  "Set it in .env (or leave RBLOG_VERSION out to follow latest, {image}:latest).":
    "Đặt trong .env (hoặc bỏ RBLOG_VERSION để luôn dùng bản mới nhất, {image}:latest).",
  "Pull the image and restart": "Tải image và khởi động lại",
  Update: "Cập nhật",
  "The container runs the database migrations when it starts, then serves. Check /health and sign in again.":
    "Khi khởi động, container chạy migration database rồi mới phục vụ. Hãy kiểm tra /health và đăng nhập lại.",
  "Roll back if needed": "Quay lại bản cũ nếu cần",
  "Put the previous version back in .env (RBLOG_VERSION={previous}) and run the same command. If the newer version migrated the database, the older one refuses to start: set RUSTCMS_DATABASE__ALLOW_NEWER_MIGRATIONS=true in rblog.env only when the release notes say its migrations only add; otherwise restore the backup.":
    "Đặt lại phiên bản cũ trong .env (RBLOG_VERSION={previous}) rồi chạy cùng lệnh. Nếu bản mới đã migrate database, bản cũ sẽ không chịu khởi động: chỉ đặt RUSTCMS_DATABASE__ALLOW_NEWER_MIGRATIONS=true trong rblog.env khi ghi chú phát hành nói migration của nó chỉ thêm mới; nếu không, hãy khôi phục bản sao lưu.",
  "Restore the backup (RBlog stopped meanwhile)": "Khôi phục bản sao lưu (tạm dừng RBlog trong lúc đó)",
  "Deployment guide": "Hướng dẫn triển khai",
});

export {};
