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
});

export {};
