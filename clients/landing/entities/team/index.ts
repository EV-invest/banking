import { ASSETS } from "@/shared/config/assets";

export type TeamMember = {
  photo: string;
  name: string;
  role: string;
  bio: string;
};

// Named team members — name lives once and feeds both the heading and the image alt text.
export const TEAM: TeamMember[] = [
  {
    photo: ASSETS.team_member_1,
    name: "Elisey TODO",
    role: "Managing Partner, Co-founder",
    bio: "15+ years managing sovereign funds across Vietnam and Singapore. Former Investment Director at VinaCapital.", //TODO: .
  },
  {
    photo: ASSETS.team_member_2,
    name: "Valeriy Sakharov",
    role: "Director of Research & Risk, CTO",
    bio: "Specializes in algorithmically-driven risk assessment and financial modelling. Previously at QuantM Alpha.",
  },
];
