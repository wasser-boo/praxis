# Default workflow - Understand, Plan, Code, Review, Test, Done
name = default

[understand]
task_template = tasks/plan
role_template = roles/senior_dev
transition -> plan on next
auto_rule: turn > 10 -> next

[plan]
task_template = tasks/plan
transition -> code on next
auto_rule: turn > 15 -> next

[code]
task_template = tasks/code
transition -> review on next
auto_rule: turn > 20 -> next

[review]
task_template = tasks/review
transition -> test on next

[test]
task_template = tasks/test
transition -> done on done

[done]
task_template = tasks/done
