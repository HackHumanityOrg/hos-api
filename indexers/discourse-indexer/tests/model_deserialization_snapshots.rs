use discourse_indexer::domain::categories::model::CategoryResponse;
use discourse_indexer::domain::likes::model::PostLikeResponse;
use discourse_indexer::domain::posts::model::PostResponse;
use discourse_indexer::domain::topics::model::TopicResponse;
use discourse_indexer::domain::users::model::UserResponse;
use serde_json::json;

/// Validates a minimal, stable snapshot for TopicResponse parsing.
#[test]
fn test_topic_response_shape_snapshot() {
    let fixture = json!({
        "topic_list": {
            "per_page": 30,
            "topics": [{
                "id": 1,
                "title": "Welcome",
                "fancy_title": "Welcome",
                "slug": "welcome",
                "posts_count": 2,
                "reply_count": 1,
                "highest_post_number": 2,
                "image_url": null,
                "created_at": "2024-08-01T00:00:00Z",
                "last_posted_at": "2024-08-02T00:00:00Z",
                "bumped": null,
                "bumped_at": "2024-08-02T00:00:00Z",
                "archetype": "regular",
                "unseen": null,
                "last_read_post_number": null,
                "unread_posts": 0,
                "pinned": null,
                "unpinned": null,
                "visible": null,
                "closed": null,
                "archived": null,
                "notification_level": null,
                "bookmarked": null,
                "liked": null,
                "views": 10,
                "like_count": 2,
                "has_summary": null,
                "last_poster_username": null,
                "category_id": 1,
                "op_like_count": null,
                "pinned_globally": null,
                "featured_link": null
            }]
        }
    });
    let tr: TopicResponse = serde_json::from_value(fixture).unwrap();
    let summary = json!({
        "first": {
            "id": tr.topic_list.topics[0].id,
            "pinned": tr.topic_list.topics[0].pinned,
            "pinned_globally": tr.topic_list.topics[0].pinned_globally,
            "slug": tr.topic_list.topics[0].slug
        },
        "per_page": tr.topic_list.per_page
    });
    let expected = json!({
        "first": {
            "id": 1,
            "pinned": null,
            "pinned_globally": null,
            "slug": "welcome"
        },
        "per_page": 30
    });
    assert_eq!(summary, expected);
}

/// Validates a minimal, stable snapshot for PostResponse parsing.
#[test]
fn test_post_response_shape_snapshot() {
    let post_json = r#"{
        "posts_count": 2,
        "post_stream": {"posts": [{
            "id": 11,
            "name": null,
            "username": "system",
            "avatar_template": "/user_avatar/meta.discourse.org/system/{size}/1.png",
            "created_at": "2024-08-01T00:00:00Z",
            "cooked": "<p>Hello</p>",
            "post_number": 1,
            "post_type": 1,
            "updated_at": "2024-08-01T00:00:00Z",
            "reply_count": 0,
            "reply_to_post_number": null,
            "quote_count": 0,
            "incoming_link_count": 0,
            "reads": 10,
            "readers_count": 10,
            "score": 1.0,
            "yours": false,
            "topic_id": 1,
            "topic_slug": "welcome",
            "display_username": null,
            "primary_group_name": null,
            "flair_name": null,
            "flair_url": null,
            "flair_bg_color": null,
            "flair_color": null,
            "version": 10,
            "can_edit": false,
            "can_delete": false,
            "can_recover": false,
            "can_wiki": false,
            "read": false,
            "user_title": null,
            "bookmarked": false,
            "moderator": false,
            "admin": false,
            "staff": false,
            "user_id": 1,
            "hidden": false,
            "trust_level": 4,
            "deleted_at": null,
            "user_deleted": false,
            "edit_reason": null,
            "can_view_edit_history": false,
            "wiki": false,
            "reviewable_id": null,
            "reviewable_score_count": null,
            "reviewable_score_pending_count": null,
            "can_see_hidden_post": null,
            "raw": null
        }]}
    }"#;
    let fixture: serde_json::Value = serde_json::from_str(post_json).unwrap();
    let pr: PostResponse = serde_json::from_value(fixture).unwrap();
    let p = &pr.post_stream.posts[0];
    let summary = json!({
        "id": p.id,
        "post_number": p.post_number,
        "user": {"username": p.username, "user_id": p.user_id},
        "topic": {"id": p.topic_id, "slug": p.topic_slug},
        "version": p.version,
        "can_edit_history": p.can_view_edit_history
    });
    let expected = json!({
        "can_edit_history": false,
        "id": 11,
        "post_number": 1,
        "topic": {"id": 1, "slug": "welcome"},
        "user": {"user_id": 1, "username": "system"},
        "version": 10
    });
    assert_eq!(summary, expected);
}

/// Validates a minimal, stable snapshot for CategoryResponse parsing.
#[test]
fn test_category_response_shape_snapshot() {
    let cat_json = r#"{
        "category_list": {
            "categories": [{
                "id": 124,
                "name": "General",
                "color": "25AAE2",
                "text_color": "000000",
                "slug": "general",
                "topic_count": 202,
                "post_count": 1759,
                "position": 1,
                "description": "Create topics here that don't fit into any other existing category.",
                "description_text": "Create topics here that don't fit into any other existing category.",
                "description_excerpt": "Create topics here...",
                "topic_url": "/t/category-definition-for-general/124",
                "read_restricted": false,
                "permission": null,
                "notification_level": null,
                "can_edit": null,
                "topic_template": null,
                "has_children": false,
                "subcategory_count": 0,
                "sort_order": null,
                "sort_ascending": null,
                "show_subcategory_list": false,
                "num_featured_topics": null,
                "default_view": null,
                "subcategory_list_style": "rows_with_featured_topics",
                "default_top_period": "all",
                "default_list_filter": "all",
                "minimum_required_tags": null,
                "navigate_to_first_post_after_read": false,
                "topics_day": 0,
                "topics_week": 1,
                "topics_month": 1,
                "topics_year": 36,
                "topics_all_time": 202,
                "uploaded_logo": null,
                "uploaded_logo_dark": null,
                "uploaded_background": null,
                "uploaded_background_dark": null,
                "is_uncategorized": false,
                "subcategory_list": []
            }]
        }
    }"#;
    let fixture: serde_json::Value = serde_json::from_str(cat_json).unwrap();
    let cr: CategoryResponse = serde_json::from_value(fixture).unwrap();
    let c = &cr.category_list.categories[0];
    assert_eq!(c.id, 124);
    assert_eq!(c.slug, "general");
    assert_eq!(c.topic_count, 202);
    assert_eq!(c.post_count, 1759);
    assert!(c.description.is_some());
    assert!(c.description_text.is_some());
}

/// Validates minimal shapes for directory users and likes responses.
#[test]
fn test_user_and_likes_response_shape_snapshot() {
    // Directory users shape
    let users_fixture = json!({
        "directory_items": [{
            "id": 1,
            "likes_received": 7,
            "likes_given": 5,
            "topics_entered": 20,
            "topic_count": 10,
            "post_count": 5,
            "posts_read": 100,
            "days_visited": 30,
            "user": {
                "id": 136681,
                "username": "xbd",
                "name": "",
                "avatar_template": "https://avatars.discourse-cdn.com/v4/letter/x/b5e925/{size}.png",
                "title": null
            }
        }]
    });
    let ur: UserResponse = serde_json::from_value(users_fixture).unwrap();
    let item = &ur.directory_items[0];
    assert_eq!(item.likes_received, 7);
    assert_eq!(item.user.id, 136681);
    assert_eq!(item.user.username, "xbd");

    // Likes shape
    let likes_fixture = json!({
        "post_action_users": [
            {
                "id": 830,
                "username": "Ray",
                "name": null,
                "avatar_template": null,
                "post_url": null,
                "username_lower": null
            },
            {
                "id": 1040,
                "username": "User",
                "name": null,
                "avatar_template": null,
                "post_url": null,
                "username_lower": null
            }
        ]
    });
    let lr: PostLikeResponse = serde_json::from_value(likes_fixture).unwrap();
    assert_eq!(lr.post_action_users.len(), 2);
    assert_eq!(lr.post_action_users[0].id, 830);
}
